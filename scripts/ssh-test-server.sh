#!/usr/bin/env bash
# Starts a private OpenSSH server for the `nmc serve install` test and
# prints the variable the test reads:
#
#   set -a; eval "$(scripts/ssh-test-server.sh /var/tmp/nmc-ssh)"; set +a
#   cargo test -p nmc-cli --test install
#
# The server listens on 127.0.0.1:2299 and lets the current user in with a
# new key. It adds a `Host nmc-test` entry to ~/.ssh/config (the test
# connects to "nmc-test") and puts a fake `systemctl` and `loginctl` first
# in the PATH of its sessions: where there is no systemd user manager (a
# container, a CI runner) the fake runs the unit's ExecStart in the
# background, which is all the install needs to be checked end to end.
#
# SSH_VARIANT picks what the nightly CI tests besides the default key login:
#   passphrase       the key has a passphrase and is loaded in an ssh-agent
#   changed-hostkey  ssh checks the host key against a known_hosts that has a
#                    different one (a changed key must be refused)
set -euo pipefail

dir=${1:?usage: ssh-test-server.sh <work dir>}
port=2299
mkdir -p "$dir/bin" "$HOME/.ssh"
chmod 700 "$HOME/.ssh"
dir=$(cd "$dir" && pwd)

variant=${SSH_VARIANT:-default}
passphrase=
[ "$variant" = passphrase ] && passphrase=nmc-key-passphrase
[ -f "$dir/id_ed25519" ] || ssh-keygen -q -t ed25519 -N "$passphrase" -f "$dir/id_ed25519"
[ -f "$dir/host_key" ] || ssh-keygen -q -t ed25519 -N '' -f "$dir/host_key"
cp "$dir/id_ed25519.pub" "$dir/authorized_keys"

cat > "$dir/bin/systemctl" <<'FAKE'
#!/bin/sh
# Stand-in for `systemctl --user` without a systemd user manager.
unit="$HOME/.config/systemd/user/nmc-serve.service"
pidfile="$HOME/.cache/nmc-fake-systemd.pid"
stop() {
    [ -f "$pidfile" ] && kill "$(cat "$pidfile")" 2>/dev/null
    rm -f "$pidfile"
}
case " $* " in
    *" restart "*)
        stop
        command=$(sed -n 's/^ExecStart=//p' "$unit" | sed "s|%h|$HOME|g")
        mkdir -p "$HOME/.cache"
        setsid sh -c "exec $command" >/dev/null 2>&1 </dev/null &
        echo $! > "$pidfile"
        ;;
    *" disable "*|*" stop "*) stop ;;
esac
exit 0
FAKE
printf '#!/bin/sh\nexit 0\n' > "$dir/bin/loginctl"
chmod +x "$dir/bin/systemctl" "$dir/bin/loginctl"

cat > "$dir/sshd_config" <<CONF
ListenAddress 127.0.0.1
Port $port
HostKey $dir/host_key
PidFile $dir/sshd.pid
AuthorizedKeysFile $dir/authorized_keys
StrictModes no
PasswordAuthentication no
UsePAM no
SetEnv PATH=$dir/bin:/usr/local/bin:/usr/bin:/bin
Subsystem sftp internal-sftp
CONF

if [ -f "$dir/sshd.pid" ] && kill -0 "$(cat "$dir/sshd.pid")" 2>/dev/null; then
    kill "$(cat "$dir/sshd.pid")"
    sleep 0.5
fi
mkdir -p /run/sshd 2>/dev/null || true
"$(command -v sshd || echo /usr/sbin/sshd)" -f "$dir/sshd_config" -E "$dir/sshd.log"

strict=no
if [ "$variant" = changed-hostkey ]; then
    strict=yes
    # Another key for the same address.
    rm -f "$dir/other_key" "$dir/other_key.pub"
    ssh-keygen -q -t ed25519 -N '' -f "$dir/other_key"
    echo "[127.0.0.1]:$port $(cut -d' ' -f1,2 "$dir/other_key.pub")" > "$dir/known_hosts"
else
    : > "$dir/known_hosts"
fi

if ! grep -q '^# nmc-test$' "$HOME/.ssh/config" 2>/dev/null; then
    cat >> "$HOME/.ssh/config" <<CONF

# nmc-test
Host nmc-test
    HostName 127.0.0.1
    Port $port
    IdentityFile $dir/id_ed25519
    IdentitiesOnly yes
    UserKnownHostsFile $dir/known_hosts
    StrictHostKeyChecking ${strict}
    LogLevel ERROR
CONF
    chmod 600 "$HOME/.ssh/config"
fi

if [ "$variant" = passphrase ]; then
    # Load the key into an agent without a terminal.
    eval "$(ssh-agent -s)" >/dev/null
    printf '#!/bin/sh\necho %s\n' "$passphrase" > "$dir/askpass"
    chmod +x "$dir/askpass"
    SSH_ASKPASS="$dir/askpass" SSH_ASKPASS_REQUIRE=force ssh-add "$dir/id_ed25519" </dev/null >/dev/null 2>&1
    echo "SSH_AUTH_SOCK=$SSH_AUTH_SOCK"
fi
if [ "$variant" != changed-hostkey ]; then
    for _ in 1 2 3 4 5 6 7 8 9 10; do
        ssh -o BatchMode=yes nmc-test true 2>/dev/null && break
        sleep 0.5
    done
    ssh -o BatchMode=yes nmc-test true
fi
echo "NMC_TEST_SSH_HOST=nmc-test"
echo "NMC_TEST_SSH_VARIANT=$variant"
