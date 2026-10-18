# Cordel — Visão do Projeto

**Status:** visão de produto para a primeira versão de testes e sua evolução.

**Idioma:** português. A documentação técnica do projeto permanece em inglês.

## Propósito

Cordel é uma plataforma de comunicação auto-hospedada para uma comunidade conversar por texto e voz e compartilhar a tela. O produto deve funcionar bem para pessoas em casas, redes e países diferentes, inclusive quando clientes estiverem atrás de NAT ou CGNAT. O primeiro servidor de testes ficará no Azure East US.

O objetivo inicial é uma experiência simples, confiável e controlada pela própria comunidade. O servidor da aplicação continua responsável por identidade, autorização, histórico de mensagens e admissão nas salas de mídia. O encaminhamento de voz e tela usa um SFU. Uma arquitetura de mídia peer-to-peer não faz parte da primeira versão.

## Primeira versão de testes

- Há **uma comunidade**, no sentido de um servidor do Discord. Dentro dela, membros podem criar canais de texto e canais de voz. A estrutura deve permitir várias comunidades em uma versão futura, sem exigir que elas existam agora.
- O acesso é **somente por convite de um admin**. Não há cadastro público. O convidado faz um registro curto com login e senha antes de entrar na comunidade. E-mail e outros dados de perfil ficam para versões posteriores. Toda pessoa precisa se autenticar com login e senha.
- O servidor da aplicação não terá interface gráfica própria: os clientes oferecem a interface do produto, inclusive os controles administrativos. Existem apenas dois níveis de acesso no produto: **user** e **admin**. Todos entram no cliente no modo comum de membro. Um admin designado pode ativar o **Management Mode** em Settings com a própria senha; o servidor emite uma autorização temporária vinculada à conta e à sessão. Não existe senha compartilhada de **superadmin**. Todos os membros podem criar canais de texto e voz, mas users não podem alterar as configurações da comunidade/servidor nem administrar membros. Na primeira versão, admins com autorização temporária válida podem alterar o nome e a descrição da comunidade, convidar, expulsar temporariamente e banir users; admins não podem aplicar kick ou ban a outros admins. O servidor valida a função e a autorização temporária em cada operação, independentemente da interface do cliente. Configurações de infraestrutura e um mecanismo separado para adicionar, remover e recuperar admins exigem acesso autorizado ao host. Um sistema detalhado de cargos e permissões por canal fica para versões futuras.
- Um **kick** impede a volta por pelo menos 30 minutos; o admin pode escolher um período maior. Um **ban** não expira automaticamente, mas um admin pode removê-lo. Uma queda de conexão não é kick: o cliente autorizado deve tentar voltar rapidamente e recuperar o estado perdido.
- Chat persistente, voz em tempo real e compartilhamento de tela devem funcionar juntos. A interface **Journal** mostra pessoas conectadas e suas salas de voz, permite ocultar e restaurar a conversa, apresenta texto formatado e imagens no chat, exibe dois compartilhamentos de tela ao mesmo tempo e permite destacar um deles em outra janela sem sair da sala. Na primeira versão não há gravação ou transcodificação de mídia como requisito.
- O cliente nativo usa Rust. **Linux é a primeira prioridade**, incluindo validação separada de X11 e Wayland. **macOS vem em seguida** e **Windows depois**. O objetivo é suportar os três, com captura de tela, áudio e distribuição tratados conforme cada sistema operacional.
- Haverá um **cliente web para testes**. Ele não precisa ter paridade com o cliente nativo. A decisão de transformá-lo em produto permanente será tomada após os testes.
- A primeira versão de testes **não exige criptografia ponta a ponta (E2EE)**. O projeto deve reservar uma evolução concreta para E2EE de texto, imagens no chat, voz e compartilhamento de tela **antes da versão final**. TLS e a criptografia de transporte da mídia não substituem E2EE.

## Escala e implantação inicial

O cenário de planejamento é **até 15 pessoas conectadas e dois compartilhamentos de tela simultâneos**. É um exemplo para dimensionar e testar o primeiro servidor, não um limite permanente nem uma garantia de desempenho. A implantação inicial usa uma VM Linux no Azure East US. Outra nuvem ou um servidor doméstico com conectividade pública adequada continuam possíveis em uma implantação futura. O mesmo host executa a aplicação, banco de dados, SFU e entrada TLS; os dados persistentes precisam de backup fora do host.

Usuários em diferentes países se conectam ao servidor em East US. O tráfego de mídia passa por ele. A experiência internacional depende de medições reais de latência, perda de pacotes e largura de banda. Quando o servidor estiver em uma residência, é necessário verificar IP público, CGNAT, portas e velocidade de upload.

## Caminho de desenvolvimento

1. **Validar riscos técnicos no Linux:** compilar o cliente e os serviços, demonstrar áudio bidirecional, captura de tela, exibição simultânea de duas telas remotas e destaque de uma delas em outra janela. Medir a usabilidade e o desempenho da interface. X11 e Wayland exigem testes separados. O resultado decide se a escolha de UI e captura é viável.
2. **Entregar identidade e comunidade:** iniciar a primeira comunidade e seu primeiro admin com um procedimento seguro; admins emitem convites; convidados criam login e senha; membros autenticados criam canais. Aplicar as regras de user/admin desde o começo, inclusive em REST e WebSocket.
3. **Entregar chat confiável:** persistir mensagens em ordem por canal, confirmar envios e recuperar eventos perdidos após desconexão. Incluir texto formatado e imagens protegidas, com armazenamento e backup aprovados antes de habilitar imagens. Presença e indicação de digitação são estados temporários.
4. **Entregar voz e tela:** admitir apenas membros autorizados nas salas de voz, publicar microfone e tela, mostrar claramente as pessoas conectadas e exibir dois compartilhamentos simultâneos, inclusive com um deles em janela separada. Validar NAT, CGNAT, TURN e reconexão com clientes em redes distintas.
5. **Validar a implantação em East US:** medir o cenário de 15 pessoas e duas telas, segurança, backups e restauração. Criar o cliente web de testes para exercitar os mesmos contratos de servidor e mídia.
6. **Ampliar suporte e preparar a versão final:** validar macOS e depois Windows; definir e implementar E2EE com gestão de chaves e recuperação apropriadas; revisar as necessidades reais de capacidade, experiência e distribuição.

Essas etapas são objetivos, não um cronograma fechado. O teste da captura, da renderização de vídeo e da experiência de chat pode exigir mudança de ferramenta antes da implementação completa.

Os testes técnicos de componentes e contratos acontecem durante a implementação dos ADRs e são necessários para concluí-los. O **teste integrado do produto** começa somente quando todos os ADRs obrigatórios para o primeiro teste Linux/web estiverem concluídos e documentados no [controle de implementação](./docs/adr/STATUS.md). Os ADRs de macOS, Windows e E2EE têm marcos posteriores e não bloqueiam esse primeiro teste.

## Evolução prevista

- Várias comunidades por instalação, com convites e administração separados.
- Cargos e controles detalhados de acesso por comunidade e canal.
- Decisão sobre manter e ampliar o cliente web.
- Outros tipos de anexos além de imagens no chat, reações, mensagens diretas, busca e notificações conforme a prioridade observada nos testes.
- Escala para mais participantes ou múltiplos hosts somente após medição da demanda e do custo operacional.

## Decisões ainda abertas

A licença do repositório oficial precisa ser compatível com as dependências escolhidas, especialmente a ferramenta de UI. Ainda faltam o procedimento seguro de recuperação de conta sem e-mail, os detalhes do mecanismo de gestão de admins no host e o desenho de chaves e recuperação da E2EE. Essas decisões devem ser registradas antes de implementar as partes correspondentes.

Os detalhes de implantação, portas, fluxos de conectividade, riscos e controles de segurança estão em [architecture.md](./architecture.md). As decisões de engenharia estão no [índice de ADRs](./docs/adr/README.md), separadas por servidor e cliente. O [mapa de trabalho](./docs/adr/WORKSTREAMS.md), o [contrato v1](./docs/contracts/v1.md) e a [especificação textual da interface Journal](./docs/ux/journal.md) permitem contribuições independentes sem depender de conversas ou mocks externos. Questões adiadas e limites ainda não resolvidos estão em [PARKING_LOT.md](./PARKING_LOT.md).
