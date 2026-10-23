# Home LXC: identity, LiveKit, and a two-network proof

Brazilian Portuguese: [README-home-lxc.pt-BR.md](README-home-lxc.pt-BR.md).

This guide is for the `Cordel` LXC (`172.16.1.16`, Ubuntu 24.04), instead of the Azure VM.
Scripts with no profile keep the Azure addresses. This host requires the profile below.
Do not record the public IP, the DuckDNS token, or passwords in this repository.

The Linux user `cordel` (`/home/cordel`) builds the binaries. The API service uses another
account, `cordel-api`, with home `/var/lib/cordel-identity`.

Replace `NOME` with the DuckDNS subdomain (only the label before `.duckdns.org`).
The expected checkout is `/home/cordel/Projects/Cordel`, on the branch that contains this file.

## Prerequisites

- Rust on `PATH` (`rustc` and `cargo`). Section 7 builds the identity API with `cargo`, and section 9 builds the Phase 0 client the same way. If `cargo` is missing, install [rustup](https://rustup.rs/) and open a new shell.
- A clone of this repository at `/home/cordel/Projects/Cordel`.
- Proxmox access to the CT, access to the router, and a DuckDNS account (section 4).

```sh
command -v cargo >/dev/null || curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
. "$HOME/.cargo/env"
cargo --version
mkdir -p /home/cordel/Projects
git clone https://github.com/Antorsrj/Cordel.git /home/cordel/Projects/Cordel
cd /home/cordel/Projects/Cordel
```

If the directory already exists, skip the clone and check out the branch that contains this file.

## 1. Proxmox

On the Proxmox **host** (the web UI, or the host shell, not inside the CT):

1. Open the `Cordel` CT → Options → Features.
2. Set **nesting** = 1. Leave the CT unprivileged.
3. Confirm the disk has several free GB and the DHCP reservation `172.16.1.16`.
4. Reboot the CT from Proxmox.

Inside the CT:

```sh
df -h /
sudo systemd-run --wait --pipe -p ProtectSystem=strict -p PrivateTmp=yes /bin/true
```

`systemd-run` must finish without error `226/NAMESPACE`. If it fails, nesting did not stick: go back to step 2 and reboot from Proxmox again.

## 2. Docker and the CT firewall

An idle Docker still punches through UFW. Before bootstrap:

```sh
sudo systemctl disable --now docker.service docker.socket containerd.service
```

Bootstrap enables UFW, allows SSH, and opens only these ports on `eth0` toward `172.16.1.16`:
TCP 80, 443, 7881, 5349 and UDP 7882, 3478. It denies 7880, 3000, 3001, and 5432.
PostgreSQL, the API (`3000`), the gate (`3001`), and LiveKit signaling (`7880`) stay on `127.0.0.1`.

## 3. Router

Reserve DHCP for the CT MAC at `172.16.1.16`. Forward only this to `172.16.1.16`:

| Port | Protocol | Role |
| --- | --- | --- |
| 80 | TCP | HTTP and the ACME challenge |
| 443 | TCP | HTTPS and `wss` signaling (Caddy) |
| 7881 | TCP | ICE/TCP |
| 7882 | UDP | UDP media (single port) |
| 3478 | UDP | TURN/UDP |
| 5349 | TCP | TURN/TLS |

Do not forward 22, 3000, 3001, 5432, or 7880. There is one public IPv4, so TURN/TLS cannot use 443 (Caddy already does). The port is 5349.

## 4. DuckDNS

1. At https://www.duckdns.org create the subdomain `NOME`. The full name is `NOME.duckdns.org`.
2. On the CT: `curl -4 -fsS https://ifconfig.me; echo`
3. On the DuckDNS site, save that IP on the subdomain. Keep the token for section 8. Do not put it in git or in chat.
4. From **another network** (a phone, not the home Wi-Fi): `dig +short NOME.duckdns.org` must show the same IP.
5. Only then continue. Without DNS and without port 80, Caddy does not get a certificate.

Updating only the DuckDNS name later leaves LiveKit's `node_ip` on the old address, and media stops with no API error. Section 8 updates both.

## 5. Profile

```sh
cp /home/cordel/Projects/Cordel/deploy/native/profiles/home-lxc.env.example /home/cordel/cordel-host.env
chmod 600 /home/cordel/cordel-host.env
```

Edit `/home/cordel/cordel-host.env` and replace every `CHANGE_ME`:

- `CORDEL_MEDIA_HOST` and `CORDEL_TURN_HOST` = `NOME.duckdns.org` (the same value; caddy mode rejects a mismatch)
- `CORDEL_NODE_IP` = the public IPv4 from section 4
- `CORDEL_ACME_EMAIL` = your email (ACME expiry notices)

`host.env` and `cordel-host.env` are in `.gitignore`. Keep the filled file outside the checkout anyway. Every following command, as root, carries the profile:

```sh
export CORDEL_HOST_ENV=/home/cordel/cordel-host.env
cd /home/cordel/Projects/Cordel
```

## 6. Edge, LiveKit, and the certificate

```sh
sudo --preserve-env=CORDEL_HOST_ENV bash deploy/native/bootstrap.sh
sudo bash deploy/native/create-livekit-secret.sh
```

Wait for the certificate. `sudo journalctl -u caddy -n 80 --no-pager` should show ACME success.
Confirm a production `.crt` exists (do not copy the key anywhere):

```sh
sudo find /var/lib/caddy -type f -name 'NOME.duckdns.org.crt'
```

Caddy stores that file under `certificates/<issuer>/<name>/`. Let's Encrypt, ZeroSSL, and other ACME issuers all use that layout. `issue-turn-cert.sh` selects the newest production file, and it skips staging issuers and Caddy's internal `local` issuer. If several production files match, the script prints the path it kept.

If nothing appears: check DNS, the port 80 forward, and `sudo ufw status numbered`.
When the file exists:

```sh
sudo --preserve-env=CORDEL_HOST_ENV bash deploy/native/issue-turn-cert.sh
sudo --preserve-env=CORDEL_HOST_ENV bash deploy/native/install-livekit.sh
```

`install-livekit.sh` leaves the service stopped. Read `/etc/livekit/livekit.yaml` **on the CT** and do not paste it anywhere (it holds the secret). It must have `node_ip` equal to the public IP, `advertise_internal_ip: true`, `tls_port: 5349`, and `docker0` in the exclude list.

```sh
sudo --preserve-env=CORDEL_HOST_ENV bash deploy/native/start-livekit.sh
sudo ss -H -lntup | awk '$5 ~ /:(80|443|3000|3001|3478|5349|5432|7880|7881|7882)$/'
```

Expected: Caddy on `172.16.1.16:80` and `:443`; LiveKit on `127.0.0.1:7880`, UDP `7882`, UDP `3478`, and TCP `5349`; nothing on `0.0.0.0:3000`, `:3001`, `:5432`, or `:7880`.

## 7. Identity API

As `cordel` (not root), on this branch:

```sh
cd /home/cordel/Projects/Cordel
cargo build --release --manifest-path server/identity/Cargo.toml
sha256sum server/identity/target/release/cordel-identity
```

Keep the hash. Do not use `build-identity.sh` or `migrate-identity-test.sh` (those stay pinned to the old Azure binary).

```sh
BIN=/home/cordel/Projects/Cordel/server/identity/target/release/cordel-identity
SHA=$(sha256sum "$BIN" | awk '{print $1}')
sudo CORDEL_IDENTITY_BINARY="$BIN" CORDEL_IDENTITY_SHA256="$SHA" \
  python3 deploy/native/setup-identity-postgres.py skip-backup-gate-for-empty-test-db
sudo --preserve-env=CORDEL_HOST_ENV bash deploy/native/migrate-identity-current.sh "$BIN" "$SHA"
sudo --preserve-env=CORDEL_HOST_ENV bash deploy/native/install-identity.sh "$BIN" "$SHA"
sudo systemctl enable --now cordel-api.service
sudo systemctl is-active caddy cordel-api cordel-livekit postgresql
```

The install leaves the API stopped on purpose; `enable --now` is the start, after the credential files exist.
If the script refuses the `cordel` account, the profile is not using `cordel-api`.

Check, still on the CT (`NOME` already resolves):

```sh
curl -s -o /dev/null -w 'me %{http_code}\n' https://NOME.duckdns.org/v1/me
curl -s -o /dev/null -w 'gate %{http_code}\n' http://127.0.0.1:3001/v1/livekit/authorize
curl -4 -s -o /dev/null -w 'reg %{http_code}\n' -X POST https://NOME.duckdns.org/v1/accounts
```

Expected: `me 401`, `gate 401`, `reg 403`. The gate must not be 200 without a token.
From another network, `nc -vz -w 3 PUBLIC_IP 3000` and the same for 3001, 5432, and 7880 must fail. 443 must accept.

First admin (the password is prompted twice; do not put it on the command line). The SSH session must be a terminal, not a pipe:

```sh
sudo -i
set -a; . /root/cordel-identity-migration.env; set +a
/usr/local/bin/cordel-identity bootstrap-admin admin-casa "Cordel-Test"
exit
```

Create an invite in the UI (Management Mode) or through the API, and a second account. Do not use other people's real accounts.
Sign in with the first account and create a `text` channel named `geral` and a `voice` channel named `voz`
(`POST /v1/communities/{community_id}/channels` with `{"kind":"text","name":"geral"}`).
`GET /v1/me` returns `community_id`. `GET /v1/communities/{community_id}/channels` returns the UUIDs.
Save the text UUID in `CORDEL_CHANNEL_ID` and the voice UUID in `CORDEL_VOICE_CHANNEL_ID`.

For the moderation 403: kick the second account for 30 minutes (Management Mode).
That account's `POST /v1/voice-channels/{id}/admission` becomes 403. The gate on `3001` also
returns 403 for a room token from a generation that was already revoked; the kicked client does not join `/rtc`.

## 8. DuckDNS updater and a changing public IP

DuckDNS can follow a new public address by itself. LiveKit does not. `rtc.node_ip` stays at the value from install, ICE keeps offering the old address, and calls fail with no error from the API. The timer updates DuckDNS on every run. It rewrites `node_ip` and restarts LiveKit only when the new IPv4 was seen on two runs in a row, or it already matches the DuckDNS A record looked up before that update. A restart drops calls that are already up. At most 3 restart attempts are kept in `/var/lib/cordel/public-ip.state` for 60 minutes. A failed restart that leaves LiveKit active still counts. If that state file is unreadable, delete it and run the timer again. When that cap is hit, the timer writes a warning to the journal and leaves LiveKit running, so a VPN that changes the egress address does not restart it on every tick. The profile write is an atomic replace in the profile's own directory: the file must be a regular file owned by the owner of its parent directory, and a symlink is refused.

The token is read from the root-only file. The updater passes it to `curl` on standard input, not on the command line, and it forces IPv4 with `curl -4`.

```sh
sudo install -m 0600 /dev/null /etc/cordel/duckdns.env
sudoedit /etc/cordel/duckdns.env
```

Contents (the token is the one from the site; do not paste it into git or chat):

```
DUCKDNS_SUBDOMAIN=NOME
DUCKDNS_TOKEN=the-token
CORDEL_HOST_ENV=/home/cordel/cordel-host.env
```

`CORDEL_HOST_ENV` is optional. Set it so the profile stays aligned with `livekit.yaml`. Without it, a later manual `update-node-ip.sh` can write the old address back.

```sh
sudo bash /home/cordel/Projects/Cordel/deploy/native/install-duckdns-timer.sh
```

The output must include `DuckDNS update returned OK.` When the address is already in use it reports that `node_ip` matches. A new address that is not stable yet is reported and LiveKit stays up. A confirmed change updates `node_ip` and restarts LiveKit. Re-run this installer after updating the checkout so the copy in `/usr/local/sbin` picks up the guard. The timer repeats every five minutes. `journalctl -u cordel-duckdns.service` shows the warning when the restart cap is hit.

Without the timer, edit `CORDEL_NODE_IP`, point DuckDNS at the same address, then run the command below. That manual command applies the address once and restarts LiveKit; it does not use the cap:

```sh
sudo --preserve-env=CORDEL_HOST_ENV bash deploy/native/update-node-ip.sh
```

## 9. Two clients

The normal client **already** talks to an HTTPS origin. Do not use `--features local-smoke`.
`NativeHttpsTransport` only accepts `https://NOME.duckdns.org` with no path, user, or query.
Chat derives `wss://NOME.duckdns.org/v1/events`. Voice asks Axum for admission; the room URL
is the server's `CORDEL_LIVEKIT_URL` (`wss://NOME.duckdns.org`), already stored in the `cordel-api` unit.

On **two** graphical Linux machines (one on the home Wi-Fi, the other on a phone hotspot):

```sh
CORDEL_API_ORIGIN=https://NOME.duckdns.org \
CORDEL_CHANNEL_ID='text-channel-uuid' \
CORDEL_VOICE_CHANNEL_ID='voice-channel-uuid' \
cargo run --release --manifest-path client/phase0/Cargo.toml
```

Do not set `CORDEL_LIVEKIT_TOKEN`. Sign in with the two accounts, open the voice channel, and share the screen
from the client button. Each side must hear the other, and the remote screen must appear.

### LAN candidate

With `node_ip` set to the public IP and without `advertise_internal_ip`, LiveKit v1.13.10 advertises **only**
that public IP (pion's 1:1 NAT rewrite replaces the local address). A client on the same LAN connects only
if the router does NAT hairpin, and that applies to media and to TURN. The home profile turns on
`advertise_internal_ip` (the `mediatransportutil` commit `f234b534b095` included in v1.13.10 honors the option
when `node_ip` is manual too) and excludes `docker0`. ICE then has `172.16.1.16` **and** the public IP.
The LAN client uses the first and does not need hairpin for UDP.

HTTPS/WSS still uses the DuckDNS name. If the router does not hairpin port 443, the LAN client cannot open
the site even with the LAN candidate correct. On the LAN PC, add one line to `/etc/hosts`:

```
172.16.1.16 NOME.duckdns.org
```

Do not add that line on the hotspot PC. During a LAN call, on the CT:

```sh
sudo tcpdump -n -i eth0 -c 20 udp port 7882
```

Packets from `172.16.1.x` (the PC) are the direct path. A source of `172.16.1.1` is usually hairpin.

### Forcing each route

On the **remote client** (hotspot), with `PUB` = the public IP, only for the test session. Delete the rules at the end.

Direct UDP (nothing blocked): the call works and `tcpdump` on the CT shows UDP 7882 from the Internet.

TURN/UDP — block direct media and leave 3478:

```sh
sudo iptables -I OUTPUT -d "$PUB" -p udp --dport 7882 -j DROP
sudo iptables -I OUTPUT -d "$PUB" -p tcp --dport 7881 -j DROP
sudo iptables -I OUTPUT -d 172.16.1.16 -p udp --dport 7882 -j DROP
sudo iptables -I OUTPUT -d 172.16.1.16 -p tcp --dport 7881 -j DROP
```

Leave the room and join again. The call must return. On the CT, `sudo tcpdump -n -i eth0 -c 10 udp port 3478` shows the relay.

TURN/TLS — also block TURN/UDP:

```sh
sudo iptables -I OUTPUT -d "$PUB" -p udp --dport 3478 -j DROP
```

Join again. The client uses `turns` on port 5349. Confirm with `sudo tcpdump -n -i eth0 -c 10 tcp port 5349`.

To clear: `sudo iptables -D OUTPUT ...` with the same rules, or reboot the test client.

### What to record for issue #11

For each of the three routes: the time, which machine was on which network, whether audio worked in both
directions, whether the remote screen appeared, and what `tcpdump` showed (7882, 3478, or 5349).
This still does not close the issue: the live proof remains pending. Do not put the public IP in git.

## 10. Rollback

Stop the Cordel units. A missing unit is expected when that stage never ran:

```sh
sudo systemctl disable --now cordel-duckdns.timer cordel-turn-cert.timer cordel-api.service cordel-livekit.service || true
sudo systemctl stop cordel-duckdns.service cordel-turn-cert.service cordel-api.service cordel-livekit.service caddy.service || true
```

Restore Caddy. `/root/Cordel.caddy.pre-identity` is the Cordel file from before `install-identity.sh` added `/v1/*`. `/root/Caddyfile.pre-cordel` is the stock file bootstrap saved (`CORDEL_BACKUP_STOCK_CADDY=1`), stored mode `0600`. Restoring the stock file and removing the drop-in returns Caddy to that file. The restore below uses mode `0644` so the `caddy` user can read it.

```sh
if [[ -f /root/Cordel.caddy.pre-identity ]]; then
  sudo install -o root -g root -m 0644 /root/Cordel.caddy.pre-identity /etc/caddy/Cordel.caddy
fi
if [[ -f /root/Caddyfile.pre-cordel ]]; then
  sudo install -o root -g root -m 0644 /root/Caddyfile.pre-cordel /etc/caddy/Caddyfile
fi
sudo rm -f /etc/systemd/system/caddy.service.d/cordel.conf
sudo systemctl daemon-reload
sudo systemctl restart caddy.service
```

To undo only the identity routes, restore `/root/Cordel.caddy.pre-identity` and leave `cordel.conf` in place, then restart Caddy.

Undo the UFW rules from section 2. Delete from the highest number so the numbers do not shift, and do not delete the OpenSSH allow:

```sh
sudo ufw status numbered
sudo ufw --force delete NUMBER
```

The Cordel rules are, on `eth0` to `172.16.1.16`: allow TCP 80, 443, 7881, and 5349; allow UDP 7882 and 3478; deny TCP 7880, 3000, 3001, and 5432.

To clear every rule and turn UFW off (the state before bootstrap):

```sh
sudo ufw --force reset
sudo ufw status
```

`Status: inactive` matches a CT that has not run bootstrap. UFW is off, so LAN SSH still works. Do not enable UFW again without an OpenSSH allow.

### Start over

After the stops, the Caddy restore, and the UFW reset, remove the generated units so the scripts will run again. Bootstrap refuses to overwrite `/etc/caddy/Cordel.caddy` or the drop-in. `issue-turn-cert.sh`, `install-livekit.sh`, and `install-identity.sh` refuse when their paths already exist.

```sh
sudo rm -f \
  /etc/caddy/Cordel.caddy \
  /etc/systemd/system/caddy.service.d/cordel.conf \
  /root/Cordel.caddy.pre-identity \
  /usr/local/sbin/cordel-install-turn-cert \
  /usr/local/sbin/cordel-duckdns-update \
  /etc/systemd/system/cordel-turn-cert.service \
  /etc/systemd/system/cordel-turn-cert.timer \
  /etc/systemd/system/cordel-duckdns.service \
  /etc/systemd/system/cordel-duckdns.timer \
  /etc/systemd/system/cordel-livekit.service \
  /etc/systemd/system/cordel-api.service \
  /usr/local/bin/livekit-server \
  /usr/local/bin/cordel-identity
sudo rm -rf /etc/livekit /usr/local/lib/cordel
sudo systemctl daemon-reload
```

Leave `/etc/cordel/livekit.env`, `/etc/cordel/identity.env`, `/etc/cordel/identity.livekit.env`, `/etc/cordel/duckdns.env`, `/etc/cordel/turn-tls`, and the PostgreSQL data in place to keep the API secret, the TURN certificate, and the accounts. `create-livekit-secret.sh` will not replace an existing `livekit.env`. Delete those files only when the credentials should be discarded.

`/var/lib/cordel-identity` and the `cordel-api` account make `install-identity.sh` stop. Remove them only for a new identity install. Dropping the database discards every account:

```sh
sudo rm -rf /var/lib/cordel-identity
sudo userdel --force cordel-api || true
sudo groupdel cordel-api || true
sudo rm -f /etc/cordel/identity.env /etc/cordel/identity.livekit.env /root/cordel-identity-migration.env
sudo -u postgres dropdb --if-exists cordel_identity
sudo -u postgres dropuser --if-exists cordel_identity_app
sudo -u postgres dropuser --if-exists cordel_identity_migrator
```

Then run this guide again from section 6. Bootstrap still expects the addresses from section 1.

## 11. What only the owner does

Nobody in this repository can reach the LXC. That stays with you: nesting and the reboot in Proxmox, the router rules, the DuckDNS account, filling `cordel-host.env` and `duckdns.env`, running the commands above, choosing the admin password, and the call with two Linux machines on different networks.
