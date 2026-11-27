//! Server-to-server copies, on the server that does the work.
//!
//! The controller asks one `pnxr serve` to copy from (pull) or to (push)
//! another, handing it a token the controller signed for the other server.
//! This server connects to the other one with its own certificate and that
//! token, then runs the normal copy engine between its local files and the
//! other server, as if it were a controller itself. The data never passes
//! through the controller.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use pnxr_ops::{ConflictPolicy, ExecuteOptions, Executor, OpEvent, PlanOptions, Step, plan_copy};
use pnxr_proto::convert::error_to_status;
use pnxr_proto::pb;
use pnxr_vfs::VfsPath;
use tokio::sync::mpsc;
use tonic::Status;
use tonic::codegen::BoxStream;

use crate::server::ServeState;

/// How often progress is reported while copying.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// Starts the copy of `request` and returns its progress messages; the last
/// one has `finished` set. Dropping the stream (the controller went away)
/// cancels the copy.
pub(crate) async fn run(
    serve: Arc<ServeState>,
    request: pb::TransferRequest,
) -> Result<BoxStream<pb::TransferProgress>, Status> {
    let (host, port) =
        crate::serve::host_and_port(&request.peer).map_err(Status::invalid_argument)?;
    let conflict = parse_conflict(&request.conflict)?;
    let config =
        serve.identity.peer_client_config().map_err(|e| Status::internal(e.to_string()))?;
    let peer = crate::serve::connect(config, &host, port, Some(&request.token))
        .await
        .map_err(Status::unavailable)?;

    let registry = pnxr_providers::default_registry();
    let remote = |path: &str| {
        VfsPath::parse(&format!("scs://{host}:{port}{path}")).map_err(|e| error_to_status(&e))
    };
    let local = |path: &str| VfsPath::from_local(Path::new(path)).map_err(|e| error_to_status(&e));
    registry.insert(&remote("/")?, Arc::new(peer));
    let (sources, destination) = if request.push {
        (
            request.sources.iter().map(|p| local(p)).collect::<Result<Vec<_>, _>>()?,
            remote(&request.destination)?,
        )
    } else {
        (
            request.sources.iter().map(|p| remote(p)).collect::<Result<Vec<_>, _>>()?,
            local(&request.destination)?,
        )
    };
    let registry = Arc::new(registry);
    let plan = plan_copy(&registry, &sources, &destination, PlanOptions::default())
        .await
        .map_err(|e| error_to_status(&e))?;

    let bytes = Arc::new(AtomicU64::new(0));
    let files = Arc::new(AtomicU64::new(0));
    let counts_as_file: Vec<bool> = plan
        .steps
        .iter()
        .map(|step| matches!(step, Step::CopyFile { .. } | Step::CopySymlink { .. }))
        .collect();
    let observer = {
        let (bytes, files) = (bytes.clone(), files.clone());
        move |event: OpEvent| match event {
            OpEvent::Bytes(n) => {
                bytes.fetch_add(n, Ordering::Relaxed);
            }
            OpEvent::StepFinished { index, .. } if counts_as_file.get(index) == Some(&true) => {
                files.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
    };
    let options = ExecuteOptions { conflict, ..ExecuteOptions::default() };
    let cancel = options.cancel.clone();
    let executor = Executor::new(registry, options).with_observer(Arc::new(observer));
    let (bytes_total, files_total) = (plan.total_bytes, plan.total_files);
    let progress = move |finished: bool, failures: Vec<String>| pb::TransferProgress {
        bytes_done: bytes.load(Ordering::Relaxed),
        bytes_total,
        files_done: files.load(Ordering::Relaxed),
        files_total,
        finished,
        failures,
    };

    let (sender, receiver) = mpsc::channel(4);
    tokio::spawn(async move {
        let run = executor.run(&plan);
        tokio::pin!(run);
        let mut tick = tokio::time::interval(PROGRESS_INTERVAL);
        let result = loop {
            tokio::select! {
                result = &mut run => break result,
                _ = tick.tick() => {
                    if sender.send(Ok(progress(false, Vec::new()))).await.is_err() {
                        cancel.cancel(); // nobody is listening any more
                    }
                }
            }
        };
        let last = match result {
            Ok(report) => {
                let mut failures = plan.warnings.clone();
                failures.extend(report.failed.into_iter().map(|(_, message)| message));
                Ok(progress(true, failures))
            }
            Err(e) => Err(error_to_status(&e)),
        };
        let _ = sender.send(last).await;
    });
    let stream = futures::stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|item| (item, receiver))
    });
    Ok(Box::pin(stream))
}

/// The conflict policy named in a request (empty: fail).
fn parse_conflict(name: &str) -> Result<ConflictPolicy, Status> {
    if name.is_empty() {
        return Ok(ConflictPolicy::Fail);
    }
    let policy: ConflictPolicy = serde_json::from_value(serde_json::Value::String(name.into()))
        .map_err(|_| Status::invalid_argument(format!("unknown conflict policy {name:?}")))?;
    if policy == ConflictPolicy::Ask {
        // Nobody is there to answer.
        return Err(Status::invalid_argument("server-to-server copies cannot ask about conflicts"));
    }
    Ok(policy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflict_policies_by_name() {
        assert_eq!(parse_conflict("").unwrap(), ConflictPolicy::Fail);
        assert_eq!(parse_conflict("overwrite-if-newer").unwrap(), ConflictPolicy::OverwriteIfNewer);
        assert_eq!(parse_conflict("keep-both").unwrap(), ConflictPolicy::KeepBoth);
        assert!(parse_conflict("ask").is_err());
        assert!(parse_conflict("whatever").is_err());
    }
}
