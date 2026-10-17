# Server ADR-001 deployment record

**Status:** Not run; provider, host type, location, domain, public-address arrangement, firewall/media-port plan, and final capacity remain open inputs.

Complete this record for each disposable or first-test deployment. Do not include passwords, API secrets, private keys, or user data.

## Host and software

- Date and operator:
- Provider or home ISP; Brazil region/city:
- Linux distribution and version:
- Kernel:
- CPU allocation:
- RAM:
- Disk type and usable capacity:
- Sustained outbound capacity and measurement method:
- Docker Engine and Compose versions:
- LiveKit, Caddy, Redis, and PostgreSQL image versions/digests:
- DNS names (app, LiveKit signaling, TURN):
- Firewall/provider/router rules:

## External connectivity

| Test | Network and client | Result / evidence |
| --- | --- | --- |
| DNS resolves to intended public address | | |
| HTTPS certificate and renewal path | | |
| WSS and LiveKit signaling | | |
| ICE/UDP media | | |
| ICE/TCP fallback | | |
| TURN/UDP | | |
| TURN/TLS | | |
| Client-side CGNAT | | |
| Restrictive/corporate network | | |
| Public scan confirms PostgreSQL/internal APIs closed | | |

## Capacity run

- Load generator location, version, and client setup:
- 15 participants joined:
- Two simultaneous screen-share publishers:
- Viewers subscribed to both shares:
- Duration:
- Peak/steady CPU:
- Peak/steady RAM:
- Peak outbound bitrate:
- Packet loss and RTT:
- Client download/quality observations:
- Issues and follow-up:

## Review

- Reproduction command(s):
- Evidence location (sanitized logs/results):
- Reviewer and date:
- Decision: Pass / Fail / Retest required
