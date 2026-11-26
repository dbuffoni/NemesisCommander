// The `nmc` global, a thin layer over the Rust operations (op_nmc_*). Kept
// free of logic beyond argument checks: everything happens in Rust, where
// permissions are enforced.
"use strict";
((globalThis) => {
  const ops = Deno.core.ops;
  // Refusals arrive as `PermissionDenied` errors that scripts can tell apart.
  class PermissionDenied extends Error {
    constructor(message) {
      super(message);
      this.name = "PermissionDenied";
    }
  }
  Deno.core.registerErrorClass("PermissionDenied", PermissionDenied);
  const text = (value) => {
    if (typeof value === "string") return value;
    try {
      return JSON.stringify(value) ?? String(value);
    } catch {
      return String(value);
    }
  };
  const stringArgument = (name, value) => {
    if (typeof value !== "string") {
      throw new TypeError(`${name} must be a string`);
    }
    return value;
  };
  const join = (...parts) =>
    parts
      .filter((part) => part !== "")
      .map((part, index) => (index === 0 ? part.replace(/\/+$/, "") : part.replace(/^\/+|\/+$/g, "")))
      .join("/") || "/";
  const nmc = {
    version: ops.op_nmc_version(),
    args: ops.op_nmc_args(),
    context: Object.freeze(ops.op_nmc_context()),
    print: (...values) => ops.op_nmc_print(values.map(text).join(" ")),
    list: (path) => ops.op_nmc_list(stringArgument("path", path)),
    stat: (path) => ops.op_nmc_stat(stringArgument("path", path)),
    exists: (path) => ops.op_nmc_exists(stringArgument("path", path)),
    readText: (path) => ops.op_nmc_read_text(stringArgument("path", path)),
    writeText: (path, content) =>
      ops.op_nmc_write_text(stringArgument("path", path), stringArgument("text", content)),
    writeBytes: (path, bytes) => {
      if (!(bytes instanceof Uint8Array)) throw new TypeError("bytes must be a Uint8Array");
      return ops.op_nmc_write_bytes(stringArgument("path", path), bytes);
    },
    mkdir: (path) => ops.op_nmc_mkdir(stringArgument("path", path)),
    rename: (from, to) => ops.op_nmc_rename(stringArgument("from", from), stringArgument("to", to)),
    remove: (path) => ops.op_nmc_remove(stringArgument("path", path)),
    run: (program, args = [], options = {}) => {
      if (!Array.isArray(args)) throw new TypeError("args must be an array of strings");
      args.forEach((arg) => stringArgument("argument", arg));
      return ops.op_nmc_run(stringArgument("program", program), args, options.cwd ?? null);
    },
    copy: (sources, destination, options = {}) => {
      if (!Array.isArray(sources)) throw new TypeError("sources must be an array of strings");
      sources.forEach((source) => stringArgument("source", source));
      return ops.op_nmc_copy(sources, stringArgument("destination", destination), options.conflict ?? "fail");
    },
    join,
    basename: (path) => stringArgument("path", path).replace(/\/+$/, "").split("/").pop() ?? "",
    dirname: (path) => {
      const trimmed = stringArgument("path", path).replace(/\/+$/, "");
      const index = trimmed.lastIndexOf("/");
      return index <= 0 ? (trimmed.startsWith("/") ? "/" : ".") : trimmed.slice(0, index);
    },
  };
  Object.freeze(nmc);
  Object.defineProperty(globalThis, "nmc", { value: nmc, enumerable: true });
  // Scripts see `nmc`, not the runtime's internals.
  delete globalThis.Deno;
})(globalThis);
