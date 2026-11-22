#!/usr/bin/env bash
# Starts a private Samba server for the SMB provider tests and prints the
# environment variables the tests read. Needs smbd and root (or sudo):
#
#   set -a; eval "$(sudo scripts/smb-test-server.sh /var/tmp/nmc-smb)"; set +a
#   cargo test -p nmc-smb
#
# The server listens on 127.0.0.1:4445, shares <dir>/share as "share", and
# accepts the user "sctest" with password "sctest-pass". Files are created
# as the user who ran sudo, so the tests can check and clean them up.
# Pick a <dir> every user can reach (not a private temporary folder).
set -euo pipefail

dir=${1:?usage: smb-test-server.sh <work dir>}
user=sctest
password=sctest-pass
port=4445
owner=${SUDO_USER:-$(id -un)}

mkdir -p "$dir"/{share,state/ncalrpc,private,lock,cache,log}
id "$user" >/dev/null 2>&1 || useradd --no-create-home --shell /usr/sbin/nologin "$user"
chown "$owner" "$dir/share"
chmod 0755 "$dir"

cat > "$dir/smb.conf" <<CONF
[global]
  server role = standalone server
  interfaces = 127.0.0.1
  bind interfaces only = yes
  smb ports = $port
  disable netbios = yes
  state directory = $dir/state
  private dir = $dir/private
  lock directory = $dir/lock
  cache directory = $dir/cache
  pid directory = $dir/state
  ncalrpc dir = $dir/state/ncalrpc
  log file = $dir/log/smbd.log
  passdb backend = tdbsam:$dir/private/passdb.tdb
  server min protocol = SMB2_10
  map to guest = never
[share]
  path = $dir/share
  read only = no
  valid users = $user
  force user = $owner
  create mask = 0644
  directory mask = 0755
CONF

printf '%s\n%s\n' "$password" "$password" | smbpasswd -c "$dir/smb.conf" -s -a "$user" >/dev/null
smbd -c "$dir/smb.conf" --daemon --no-process-group 2>/dev/null || smbd -s "$dir/smb.conf" -D

# Wait for the port.
for _ in $(seq 50); do
    if (exec 3<>/dev/tcp/127.0.0.1/$port) 2>/dev/null; then break; fi
    sleep 0.1
done

echo "NMC_TEST_SMB_URL=smb://$user@127.0.0.1:$port/share"
echo "NMC_TEST_SMB_PASSWORD=$password"
echo "NMC_TEST_SMB_DIR=$dir/share"
