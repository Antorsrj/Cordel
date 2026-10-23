# LXC de casa: identidade, LiveKit e prova entre duas redes

English: [README-home-lxc.md](README-home-lxc.md).

Este guia é para o LXC `Cordel` (`172.16.1.16`, Ubuntu 24.04), no lugar da VM Azure.
Os scripts sem perfil continuam com os endereços Azure. Aqui o perfil é obrigatório.
Não grave o IP público, o token DuckDNS nem senhas neste repositório.

O usuário Linux `cordel` (`/home/cordel`) é quem compila. O serviço da API
usa outra conta, `cordel-api`, com home `/var/lib/cordel-identity`.

Substitua `NOME` pelo subdomínio DuckDNS (só o rótulo antes de `.duckdns.org`).
O checkout esperado é `/home/cordel/Projects/Cordel`, no branch que contém este arquivo.

## Pré-requisitos

- Rust no `PATH` (`rustc` e `cargo`). A seção 7 compila a API de identidade com `cargo`, e a seção 9 compila o cliente Phase 0 do mesmo jeito. Se `cargo` não existir, instale o [rustup](https://rustup.rs/) e abra um shell novo.
- Um clone deste repositório em `/home/cordel/Projects/Cordel`.
- Acesso ao Proxmox do CT, acesso ao roteador e uma conta DuckDNS (seção 4).

```sh
command -v cargo >/dev/null || curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
. "$HOME/.cargo/env"
cargo --version
mkdir -p /home/cordel/Projects
git clone https://github.com/Antorsrj/Cordel.git /home/cordel/Projects/Cordel
cd /home/cordel/Projects/Cordel
```

Se o diretório já existir, pule o clone e faça checkout do branch que contém este arquivo.

## 1. Proxmox

No **host** Proxmox (a interface web, ou o shell do host, não dentro do CT):

1. Abra o CT `Cordel` → Options → Features.
2. Marque **nesting** = 1. Deixe o CT unprivileged.
3. Confirme o disco com vários GB livres e a reserva DHCP `172.16.1.16`.
4. Reinicie o CT pelo Proxmox.

Dentro do CT:

```sh
df -h /
sudo systemd-run --wait --pipe -p ProtectSystem=strict -p PrivateTmp=yes /bin/true
```

O `systemd-run` precisa terminar sem o erro `226/NAMESPACE`. Se falhar, o nesting não entrou: volte ao passo 2 e reinicie de novo pelo Proxmox.

## 2. Docker e firewall do CT

O Docker, mesmo sem containers, ainda fura o UFW. Antes do bootstrap:

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
| 80 | TCP | HTTP e desafio ACME |
| 443 | TCP | HTTPS e sinal `wss` (Caddy) |
| 7881 | TCP | ICE/TCP |
| 7882 | UDP | mídia UDP (porta única) |
| 3478 | UDP | TURN/UDP |
| 5349 | TCP | TURN/TLS |

Não encaminhe 22, 3000, 3001, 5432 nem 7880. Há um único IPv4 público, então TURN/TLS não pode usar 443 (a Caddy já usa). Por isso a porta é 5349.

## 4. DuckDNS

1. Em https://www.duckdns.org crie o subdomínio `NOME`. O nome final é `NOME.duckdns.org`.
2. No CT: `curl -4 -fsS https://ifconfig.me; echo`
3. No site DuckDNS, grave esse IP no subdomínio. Guarde o token para a seção 8. Não o coloque no git nem no chat.
4. De **outra rede** (celular, não o Wi-Fi de casa): `dig +short NOME.duckdns.org` tem que mostrar o mesmo IP.
5. Só então avance. Sem DNS e sem a porta 80, a Caddy não obtém certificado.

Atualizar só o nome DuckDNS mais tarde deixa o `node_ip` do LiveKit no endereço antigo, e a mídia para sem erro da API. A seção 8 atualiza os dois.

## 5. Perfil

```sh
cp /home/cordel/Projects/Cordel/deploy/native/profiles/home-lxc.env.example /home/cordel/cordel-host.env
chmod 600 /home/cordel/cordel-host.env
```

Edite `/home/cordel/cordel-host.env` e troque cada `CHANGE_ME`:

- `CORDEL_MEDIA_HOST` e `CORDEL_TURN_HOST` = `NOME.duckdns.org` (os dois iguais; o modo caddy rejeita a diferença)
- `CORDEL_NODE_IP` = o IPv4 público da seção 4
- `CORDEL_ACME_EMAIL` = um e-mail seu (avisos de validade do ACME)

`host.env` e `cordel-host.env` estão no `.gitignore`. Mesmo assim, mantenha o arquivo preenchido fora do checkout. Todos os comandos seguintes, como root, levam o perfil:

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
Confirme que existe um `.crt` de produção (não copie a chave para lugar nenhum):

```sh
sudo find /var/lib/caddy -type f -name 'NOME.duckdns.org.crt'
```

A Caddy grava esse arquivo em `certificates/<issuer>/<nome>/`. Let's Encrypt, ZeroSSL e outros emissores ACME usam esse layout. O `issue-turn-cert.sh` escolhe o arquivo de produção mais novo e ignora emissores de staging e o emissor interno `local` da Caddy. Se vários arquivos de produção baterem, o script mostra o caminho que ficou.

Se não aparecer: confira o DNS, o encaminhamento da porta 80 e `sudo ufw status numbered`.
Quando o arquivo existir:

```sh
sudo --preserve-env=CORDEL_HOST_ENV bash deploy/native/issue-turn-cert.sh
sudo --preserve-env=CORDEL_HOST_ENV bash deploy/native/install-livekit.sh
```

O `install-livekit.sh` deixa o serviço parado. Veja `/etc/livekit/livekit.yaml` **no CT** e não o cole em lugar nenhum (tem o segredo). Tem que ter `node_ip` igual ao IP público, `advertise_internal_ip: true`, `tls_port: 5349` e `docker0` na lista de exclusão.

```sh
sudo --preserve-env=CORDEL_HOST_ENV bash deploy/native/start-livekit.sh
sudo ss -H -lntup | awk '$5 ~ /:(80|443|3000|3001|3478|5349|5432|7880|7881|7882)$/'
```

Esperado: Caddy em `172.16.1.16:80` e `:443`; LiveKit em `127.0.0.1:7880`, UDP `7882`, UDP `3478` e TCP `5349`; nada em `0.0.0.0:3000`, `:3001`, `:5432` ou `:7880`.

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

A instalação deixa a API parada de propósito; o `enable --now` é o que sobe o serviço, depois que os arquivos de credenciais existem.
Se o script recusar a conta `cordel`, o perfil não está usando `cordel-api`.

Verificação, ainda no CT (`NOME` já resolvido):

```sh
curl -s -o /dev/null -w 'me %{http_code}\n' https://NOME.duckdns.org/v1/me
curl -s -o /dev/null -w 'gate %{http_code}\n' http://127.0.0.1:3001/v1/livekit/authorize
curl -4 -s -o /dev/null -w 'reg %{http_code}\n' -X POST https://NOME.duckdns.org/v1/accounts
```

Esperado: `me 401`, `gate 401`, `reg 403`. O gate não pode ser 200 sem token.
De outra rede, `nc -vz -w 3 IP_PUBLICO 3000` e o mesmo para 3001, 5432 e 7880 têm que falhar. 443 tem que aceitar.

Primeiro admin (pede a senha duas vezes; não a ponha na linha de comando). A sessão SSH tem que ser um terminal, não um pipe:

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

## 8. Atualizador DuckDNS e IP público que muda

O DuckDNS consegue acompanhar um endereço público novo sozinho. O LiveKit não. O `rtc.node_ip` fica no valor da instalação, o ICE continua oferecendo o endereço antigo, e as chamadas falham sem erro da API. O timer atualiza o DuckDNS em toda execução. Ele reescreve o `node_ip` e reinicia o LiveKit só quando o IPv4 novo apareceu em duas execuções seguidas, ou quando ele já é o registro A do DuckDNS consultado antes dessa atualização. O reinício derruba chamadas que já estão no ar. No máximo 3 tentativas de reinício ficam em `/var/lib/cordel/public-ip.state` por 60 minutos. Um reinício que falha e deixa o LiveKit ativo também entra na conta. Se esse arquivo de estado estiver ilegível, apague-o e rode o timer de novo. Quando esse limite estoura, o timer grava um aviso no journal e deixa o LiveKit rodando, então uma VPN que muda o endereço de saída não reinicia o serviço a cada ciclo. A gravação do perfil é uma troca atômica no próprio diretório do perfil: o arquivo tem que ser regular e pertencer ao dono do diretório pai, e um symlink é recusado.

O token é lido do arquivo só do root. O atualizador entrega o token ao `curl` pela entrada padrão, não pela linha de comando, e força IPv4 com `curl -4`.

```sh
sudo install -m 0600 /dev/null /etc/cordel/duckdns.env
sudoedit /etc/cordel/duckdns.env
```

Conteúdo (o token é o do site; não o cole no git nem no chat):

```
DUCKDNS_SUBDOMAIN=NOME
DUCKDNS_TOKEN=o-token
CORDEL_HOST_ENV=/home/cordel/cordel-host.env
```

`CORDEL_HOST_ENV` é opcional. Defina para o perfil continuar alinhado com o `livekit.yaml`. Sem ele, um `update-node-ip.sh` manual posterior pode gravar o endereço antigo de volta.

```sh
sudo bash /home/cordel/Projects/Cordel/deploy/native/install-duckdns-timer.sh
```

A saída tem que incluir `DuckDNS update returned OK.` Quando o endereço já está em uso, ela diz que o `node_ip` bate. Um endereço novo que ainda não está estável é informado e o LiveKit continua no ar. Uma mudança confirmada atualiza o `node_ip` e reinicia o LiveKit. Rode este instalador de novo depois de atualizar o checkout para a cópia em `/usr/local/sbin` receber essa proteção. O timer repete a cada cinco minutos. `journalctl -u cordel-duckdns.service` mostra o aviso quando o limite de reinícios estoura.

Sem o timer, edite `CORDEL_NODE_IP`, aponte o DuckDNS para o mesmo endereço e rode o comando abaixo. Esse comando manual aplica o endereço uma vez e reinicia o LiveKit; ele não usa o limite:

```sh
sudo --preserve-env=CORDEL_HOST_ENV bash deploy/native/update-node-ip.sh
```

## 9. Dois clientes

O cliente normal **já** fala com uma origem HTTPS. Não use `--features local-smoke`.
`NativeHttpsTransport` só aceita `https://NOME.duckdns.org` sem caminho, usuário ou query.
O chat deriva `wss://NOME.duckdns.org/v1/events`. A voz pede a admissão ao Axum; a URL da sala
é o `CORDEL_LIVEKIT_URL` do servidor (`wss://NOME.duckdns.org`), já gravado na unit `cordel-api`.

Em **dois** Linux com ambiente gráfico (um no Wi-Fi de casa, outro num hotspot do celular):

```sh
CORDEL_API_ORIGIN=https://NOME.duckdns.org \
CORDEL_CHANNEL_ID='uuid-do-canal-de-texto' \
CORDEL_VOICE_CHANNEL_ID='uuid-do-canal-de-voz' \
cargo run --release --manifest-path client/phase0/Cargo.toml
```

Não defina `CORDEL_LIVEKIT_TOKEN`. Entre com as duas contas, abra o canal de voz e compartilhe a tela
a partir do botão do cliente. Os dois precisam se ouvir e a tela remota precisa aparecer.

### Candidato de LAN

Com `node_ip` apontado ao IP público e sem `advertise_internal_ip`, o LiveKit v1.13.10 anuncia **só**
esse IP público (o rewrite NAT 1:1 do pion substitui o endereço local). Um cliente na mesma LAN só entra
se o roteador fizer NAT hairpin, e isso vale para a mídia e para o TURN. O perfil de casa liga
`advertise_internal_ip` (o commit `f234b534b095` do `mediatransportutil` incluído no v1.13.10 respeita a
opção também quando `node_ip` é manual) e exclui `docker0`. O ICE fica com `172.16.1.16` **e** com o IP
público. O cliente da LAN usa o primeiro e não precisa de hairpin para o UDP.

O HTTPS/WSS continua usando o nome DuckDNS. Se o roteador não fizer hairpin na porta 443, o cliente da
LAN não abre o site mesmo com o candidato de LAN certo. No PC da LAN, uma linha em `/etc/hosts`:

```
172.16.1.16 NOME.duckdns.org
```

Não ponha essa linha no PC do hotspot. Confirme no CT, durante uma chamada LAN:

```sh
sudo tcpdump -n -i eth0 -c 20 udp port 7882
```

Pacotes com origem `172.16.1.x` (o PC) são caminho direto. Origem `172.16.1.1` costuma ser hairpin.

### Forçar cada rota

No **cliente remoto** (hotspot), com `PUB` = IP público, só na sessão de teste. Apague as regras no fim.

UDP direto (nada bloqueado): a chamada funciona e o `tcpdump` no CT mostra UDP 7882 vindo da Internet.

TURN/UDP — bloqueie a mídia direta e deixe o 3478:

```sh
sudo iptables -I OUTPUT -d "$PUB" -p udp --dport 7882 -j DROP
sudo iptables -I OUTPUT -d "$PUB" -p tcp --dport 7881 -j DROP
sudo iptables -I OUTPUT -d 172.16.1.16 -p udp --dport 7882 -j DROP
sudo iptables -I OUTPUT -d 172.16.1.16 -p tcp --dport 7881 -j DROP
```

Saia da sala e entre outra vez. A chamada tem que voltar. No CT, `sudo tcpdump -n -i eth0 -c 10 udp port 3478` mostra o relay.

TURN/TLS — acrescente o corte do TURN/UDP:

```sh
sudo iptables -I OUTPUT -d "$PUB" -p udp --dport 3478 -j DROP
```

Entre de novo. O cliente usa `turns` na porta 5349. Confirme com `sudo tcpdump -n -i eth0 -c 10 tcp port 5349`.

Para limpar: `sudo iptables -D OUTPUT ...` com as mesmas regras, ou reinicie o cliente de teste.

### O que anotar para a issue #11

Para cada uma das três rotas: hora, qual máquina estava em cada rede, se o áudio foi nos dois
sentidos, se a tela remota apareceu, e o que o `tcpdump` mostrou (7882, 3478 ou 5349).
Isto ainda não fecha a issue: a prova ao vivo fica pendente. Não ponha o IP público no git.

## 10. Rollback

Pare as units do Cordel. Uma unit ausente é esperada quando aquela etapa nunca rodou:

```sh
sudo systemctl disable --now cordel-duckdns.timer cordel-turn-cert.timer cordel-api.service cordel-livekit.service || true
sudo systemctl stop cordel-duckdns.service cordel-turn-cert.service cordel-api.service cordel-livekit.service caddy.service || true
```

Restaure a Caddy. `/root/Cordel.caddy.pre-identity` é o arquivo Cordel de antes de `install-identity.sh` acrescentar `/v1/*`. `/root/Caddyfile.pre-cordel` é o arquivo original que o bootstrap guardou (`CORDEL_BACKUP_STOCK_CADDY=1`), modo `0600`. Restaurar o arquivo original e remover o drop-in devolve a Caddy a esse arquivo. A restauração abaixo usa modo `0644` para o usuário `caddy` conseguir ler.

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

Para desfazer só as rotas de identidade, restaure `/root/Cordel.caddy.pre-identity` e deixe o `cordel.conf` no lugar, depois reinicie a Caddy.

Desfaça as regras de UFW da seção 2. Apague a partir do número mais alto para os números não mudarem, e não apague o allow do OpenSSH:

```sh
sudo ufw status numbered
sudo ufw --force delete NUMERO
```

As regras do Cordel são, em `eth0` para `172.16.1.16`: allow TCP 80, 443, 7881 e 5349; allow UDP 7882 e 3478; deny TCP 7880, 3000, 3001 e 5432.

Para limpar todas as regras e desligar o UFW (o estado de antes do bootstrap):

```sh
sudo ufw --force reset
sudo ufw status
```

`Status: inactive` corresponde a um CT que não rodou o bootstrap. O UFW fica desligado, então o SSH da LAN continua funcionando. Não ligue o UFW de novo sem um allow de OpenSSH.

### Começar do zero

Depois das paradas, da restauração da Caddy e do reset do UFW, remova as units geradas para os scripts aceitarem rodar de novo. O bootstrap recusa sobrescrever `/etc/caddy/Cordel.caddy` ou o drop-in. `issue-turn-cert.sh`, `install-livekit.sh` e `install-identity.sh` recusam quando os caminhos deles já existem.

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

Deixe `/etc/cordel/livekit.env`, `/etc/cordel/identity.env`, `/etc/cordel/identity.livekit.env`, `/etc/cordel/duckdns.env`, `/etc/cordel/turn-tls` e os dados do PostgreSQL no lugar para conservar o segredo da API, o certificado TURN e as contas. O `create-livekit-secret.sh` não substitui um `livekit.env` existente. Apague esses arquivos só quando as credenciais tiverem que ser descartadas.

`/var/lib/cordel-identity` e a conta `cordel-api` fazem o `install-identity.sh` parar. Remova os dois só para uma instalação nova da identidade. Apagar o banco descarta todas as contas:

```sh
sudo rm -rf /var/lib/cordel-identity
sudo userdel --force cordel-api || true
sudo groupdel cordel-api || true
sudo rm -f /etc/cordel/identity.env /etc/cordel/identity.livekit.env /root/cordel-identity-migration.env
sudo -u postgres dropdb --if-exists cordel_identity
sudo -u postgres dropuser --if-exists cordel_identity_app
sudo -u postgres dropuser --if-exists cordel_identity_migrator
```

Depois rode este guia de novo a partir da seção 6. O bootstrap ainda exige os endereços da seção 1.

## 11. O que só o dono faz

Ninguém neste repositório alcança o LXC. Fica com você: nesting e reboot no Proxmox, regras do
roteador, conta DuckDNS, preencher `cordel-host.env` e `duckdns.env`, rodar os comandos acima,
escolher a senha do admin, e a chamada com dois Linux em redes diferentes.
