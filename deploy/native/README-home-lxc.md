# LXC de casa: identidade, LiveKit e prova entre duas redes

Este guia é para o LXC `Cordel` (`172.16.1.16`, Ubuntu 24.04), no lugar da VM Azure.
Os scripts sem perfil continuam com os endereços Azure. Aqui o perfil é obrigatório.
Não grave o IP público, o token DuckDNS nem senhas neste repositório.

O usuário Linux `cordel` (`/home/cordel`) continua sendo quem compila. O serviço da API
usa outra conta, `cordel-api`, com home `/var/lib/cordel-identity`.

Substitua `NOME` pelo subdomínio DuckDNS (só a parte antes de `.duckdns.org`).
O checkout esperado é `/home/cordel/Projects/Cordel`, neste branch.

## 1. Proxmox

No **host** Proxmox (a interface web, ou o shell do host, não dentro do CT):

1. Abra o CT `Cordel` → Options → Features.
2. Marque **nesting** = 1. Deixe o CT unprivileged.
3. Confirme o disco com folga (o diagnóstico de 22/10 tinha pouco espaço; hoje precisa de vários GB livres) e a reserva DHCP `172.16.1.16`.
4. Reinicie o CT pelo Proxmox.

Dentro do CT:

```sh
df -h /
sudo systemd-run --wait --pipe -p ProtectSystem=strict -p PrivateTmp=yes /bin/true
```

O `systemd-run` tem de terminar sem erro `226/NAMESPACE`. Se falhar, o nesting não entrou: volte ao passo 2 e reinicie de novo pelo Proxmox.

## 2. Docker e firewall do CT

Docker vazio ainda fura o UFW. Antes do bootstrap:

```sh
sudo systemctl disable --now docker.service docker.socket containerd.service
```

O bootstrap liga o UFW, libera SSH e só estas portas na interface `eth0` para `172.16.1.16`:
TCP 80, 443, 7881, 5349 e UDP 7882, 3478. Nega 7880, 3000, 3001 e 5432.
PostgreSQL, a API (`3000`), o gate (`3001`) e o sinal LiveKit (`7880`) ficam em `127.0.0.1`.

## 3. Roteador

Reserva de DHCP para o MAC do CT → `172.16.1.16`. Encaminhe só isto para `172.16.1.16`:

| Porta | Protocolo | Função |
| --- | --- | --- |
| 80 | TCP | HTTP e desafio Let's Encrypt |
| 443 | TCP | HTTPS e sinal `wss` (Caddy) |
| 7881 | TCP | ICE/TCP |
| 7882 | UDP | mídia UDP (porta única) |
| 3478 | UDP | TURN/UDP |
| 5349 | TCP | TURN/TLS |

Não encaminhe 22, 3000, 3001, 5432 nem 7880. Há um único IPv4 público, então TURN/TLS não pode usar 443 (a Caddy já usa). Por isso a porta é 5349.

## 4. DuckDNS

1. Em https://www.duckdns.org crie o subdomínio `NOME`. O nome final é `NOME.duckdns.org`.
2. No CT: `curl -4 -fsS https://ifconfig.me; echo`
3. No site DuckDNS, grave esse IP no subdomínio (o token fica para o passo 12).
4. De **outra rede** (telemóvel, não o Wi-Fi de casa): `dig +short NOME.duckdns.org` tem de mostrar o mesmo IP.
5. Só então avance. Sem DNS e sem a porta 80, a Caddy não obtém certificado.

## 5. Perfil

```sh
cp /home/cordel/Projects/Cordel/deploy/native/profiles/home-lxc.env.example /home/cordel/cordel-host.env
chmod 600 /home/cordel/cordel-host.env
```

Edite `/home/cordel/cordel-host.env` e troque cada `CHANGE_ME`:

- `CORDEL_MEDIA_HOST` e `CORDEL_TURN_HOST` = `NOME.duckdns.org` (os dois iguais)
- `CORDEL_NODE_IP` = o IPv4 público do passo 4
- `CORDEL_ACME_EMAIL` = um email seu (avisos do Let's Encrypt)

Não faça `git add` desse arquivo. Todos os comandos seguintes, como root, levam o perfil:

```sh
export CORDEL_HOST_ENV=/home/cordel/cordel-host.env
cd /home/cordel/Projects/Cordel
```

## 6. Borda, LiveKit e certificado

```sh
sudo --preserve-env=CORDEL_HOST_ENV bash deploy/native/bootstrap.sh
sudo bash deploy/native/create-livekit-secret.sh
```

Espere o certificado. `sudo journalctl -u caddy -n 80 --no-pager` deve mostrar o sucesso ACME.
Confirme que existe um `.crt` de produção (não copie a chave para lado nenhum):

```sh
sudo find /var/lib/caddy -type f -path '*acme-v02.api.letsencrypt.org-directory*' -name 'NOME.duckdns.org.crt'
```

Se não aparecer: confira o DNS, o encaminhamento da porta 80 e `sudo ufw status numbered`.
Quando o ficheiro existir:

```sh
sudo --preserve-env=CORDEL_HOST_ENV bash deploy/native/issue-turn-cert.sh
sudo --preserve-env=CORDEL_HOST_ENV bash deploy/native/install-livekit.sh
```

O `install-livekit.sh` deixa o serviço parado. Veja `/etc/livekit/livekit.yaml` **no CT** e não o cole em lado nenhum (tem o segredo). Tem de ter `node_ip` igual ao IP público, `advertise_internal_ip: true`, `tls_port: 5349` e `docker0` na lista de exclusão.

```sh
sudo --preserve-env=CORDEL_HOST_ENV bash deploy/native/start-livekit.sh
sudo ss -H -lntup | awk '$5 ~ /:(80|443|3000|3001|3478|5349|5432|7880|7881|7882)$/'
```

Esperado: Caddy em `172.16.1.16:80` e `:443`; LiveKit em `127.0.0.1:7880`, UDP `7882`, UDP `3478` e TCP `5349`; nada de `0.0.0.0:3000`, `:3001`, `:5432` ou `:7880`.

## 7. API de identidade

Como `cordel` (não como root), neste branch:

```sh
cd /home/cordel/Projects/Cordel
cargo build --release --manifest-path server/identity/Cargo.toml
sha256sum server/identity/target/release/cordel-identity
```

Guarde o hash. Não use `build-identity.sh` nem `migrate-identity-test.sh` (esses ficam presos ao binário antigo da Azure).

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

A instalação deixa a API parada de propósito; o `enable --now` é o arranque, depois dos ficheiros de credenciais existirem.
Se o script recusar a conta `cordel`, o perfil não está a usar `cordel-api`.

Verificação, ainda no CT (`NOME` já resolvido):

```sh
curl -s -o /dev/null -w 'me %{http_code}\n' https://NOME.duckdns.org/v1/me
curl -s -o /dev/null -w 'gate %{http_code}\n' http://127.0.0.1:3001/v1/livekit/authorize
curl -4 -s -o /dev/null -w 'reg %{http_code}\n' -X POST https://NOME.duckdns.org/v1/accounts
```

Esperado: `me 401`, `gate 401`, `reg 403`. O gate **não** pode ser 200 sem token.
De outra rede, `nc -vz -w 3 IP_PUBLICO 3000` e o mesmo para 3001, 5432 e 7880 têm de falhar. 443 tem de aceitar.

Primeiro admin (pede a senha duas vezes; não a ponha na linha de comando). A sessão SSH tem de ser um terminal, não um pipe:

```sh
sudo -i
set -a; . /root/cordel-identity-migration.env; set +a
/usr/local/bin/cordel-identity bootstrap-admin admin-casa "Cordel-Test"
exit
```

Crie um convite na interface (Management Mode) ou pela API, e uma segunda conta. Não use contas reais de outras pessoas.
Entre com a primeira conta e crie um canal `text` chamado `geral` e um canal `voice` chamado `voz`
(`POST /v1/communities/{community_id}/channels` com `{"kind":"text","name":"geral"}`).
`GET /v1/me` traz `community_id`. `GET /v1/communities/{community_id}/channels` traz os UUIDs.
Guarde o UUID de texto em `CORDEL_CHANNEL_ID` e o de voz em `CORDEL_VOICE_CHANNEL_ID`.

Para o 403 de moderação: expulse a segunda conta por 30 minutos (Management Mode).
O `POST /v1/voice-channels/{id}/admission` dessa conta passa a 403. O gate em `3001` também
responde 403 a um token de sala de uma geração já revogada; o cliente expulso não entra em `/rtc`.

## 8. Atualizador DuckDNS (opcional, recomendado)

```sh
sudo install -m 0600 /dev/null /etc/cordel/duckdns.env
sudoedit /etc/cordel/duckdns.env
```

Conteúdo (o token é o do site; não o cole no git nem no chat):

```
DUCKDNS_SUBDOMAIN=NOME
DUCKDNS_TOKEN=o-token
```

```sh
sudo bash /home/cordel/Projects/Cordel/deploy/native/install-duckdns-timer.sh
```

A saída deve ser `DuckDNS update returned OK.` Se o IP público mudar mais tarde:

```sh
sudo --preserve-env=CORDEL_HOST_ENV bash deploy/native/update-node-ip.sh
```

(atualize antes o `CORDEL_NODE_IP` no `cordel-host.env` e o DuckDNS).

## 9. Dois clientes

O cliente normal **já** fala com um origem HTTPS. Não use `--features local-smoke`.
`NativeHttpsTransport` só aceita `https://NOME.duckdns.org` sem caminho, utilizador ou query.
O chat deriva `wss://NOME.duckdns.org/v1/events`. A voz pede a admissão ao Axum; o URL da sala
é o `CORDEL_LIVEKIT_URL` do servidor (`wss://NOME.duckdns.org`), já gravado na unit `cordel-api`.

Em **dois** Linux com ambiente gráfico (um no Wi-Fi de casa, outro num hotspot do telemóvel):

```sh
CORDEL_API_ORIGIN=https://NOME.duckdns.org \
CORDEL_CHANNEL_ID='uuid-do-canal-de-texto' \
CORDEL_VOICE_CHANNEL_ID='uuid-do-canal-de-voz' \
cargo run --release --manifest-path client/phase0/Cargo.toml
```

Não defina `CORDEL_LIVEKIT_TOKEN`. Entre com as duas contas, abra o canal de voz e partilhe o ecrã
a partir do botão do cliente. Os dois têm de se ouvir e o ecrã remoto tem de aparecer.

### Candidato de LAN

Com `node_ip` apontado ao IP público e sem `advertise_internal_ip`, o LiveKit v1.13.10 **só**
anuncia esse IP público (o rewrite NAT 1:1 do pion substitui o endereço local). Um cliente na
mesma LAN só entra se o roteador fizer NAT hairpin, e isso vale para a mídia e para o TURN.
O perfil de casa liga `advertise_internal_ip` (o commit `f234b534b095` do `mediatransportutil`
incluído no v1.13.10 respeita a opção também quando `node_ip` é manual) e exclui `docker0`.
O ICE fica com `172.16.1.16` **e** com o IP público. O cliente da LAN usa o primeiro e não precisa
de hairpin para o UDP.

O HTTPS/WSS continua a usar o nome DuckDNS. Se o roteador não fizer hairpin na porta 443, o
cliente da LAN não abre o site mesmo com o candidato de LAN correcto. Solução: no PC da LAN,
uma linha em `/etc/hosts`:

```
172.16.1.16 NOME.duckdns.org
```

Não ponha essa linha no PC do hotspot. Confirme no CT, durante uma chamada LAN:

```sh
sudo tcpdump -n -i eth0 -c 20 udp port 7882
```

Pacotes com origem `172.16.1.x` (o PC) são caminho directo. Origem `172.16.1.1` costuma ser hairpin.

### Forçar cada rota

No **cliente remoto** (hotspot), com `PUB` = IP público, só na sessão de teste. Apague as regras no fim.

UDP directo (nada bloqueado): a chamada funciona e o `tcpdump` no CT mostra UDP 7882 vindo da Internet.

TURN/UDP — bloqueie a mídia directa e deixe o 3478:

```sh
sudo iptables -I OUTPUT -d "$PUB" -p udp --dport 7882 -j DROP
sudo iptables -I OUTPUT -d "$PUB" -p tcp --dport 7881 -j DROP
sudo iptables -I OUTPUT -d 172.16.1.16 -p udp --dport 7882 -j DROP
sudo iptables -I OUTPUT -d 172.16.1.16 -p tcp --dport 7881 -j DROP
```

Saia da sala e entre outra vez. A chamada tem de voltar. No CT, `sudo tcpdump -n -i eth0 -c 10 udp port 3478` mostra o relay.

TURN/TLS — acrescente o corte do TURN/UDP:

```sh
sudo iptables -I OUTPUT -d "$PUB" -p udp --dport 3478 -j DROP
```

Entre de novo. O cliente usa `turns` na porta 5349. Confirme com `sudo tcpdump -n -i eth0 -c 10 tcp port 5349`.

Para limpar: `sudo iptables -D OUTPUT ...` com as mesmas regras, ou reinicie o cliente de teste.

### O que anotar para a issue #11

Para cada uma das três rotas: hora, qual máquina estava em cada rede, se o áudio foi nos dois
sentidos, se o ecrã remoto apareceu, e o que o `tcpdump` mostrou (7882, 3478 ou 5349).
Isto ainda não fecha a issue: a prova ao vivo fica pendente. Não ponha o IP público no git.

## 10. O que só o dono faz

Ninguém neste repositório alcança o LXC. Fica consigo: nesting e reboot no Proxmox, regras do
roteador, conta DuckDNS, preencher `cordel-host.env` e `duckdns.env`, correr os comandos acima,
escolher a senha do admin, e a chamada com dois Linux em redes diferentes.
