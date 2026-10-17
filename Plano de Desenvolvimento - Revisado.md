# Cordel — Visão do Projeto

**Status:** visão de produto para a primeira versão de testes e sua evolução.

**Idioma:** português. A documentação técnica do projeto permanece em inglês.

## Propósito

Cordel é uma plataforma de comunicação auto-hospedada para uma comunidade conversar por texto e voz e compartilhar a tela. O produto deve funcionar bem para pessoas em casas, redes e países diferentes, inclusive quando clientes estiverem atrás de NAT ou CGNAT. O primeiro servidor de testes ficará no Brasil.

O objetivo inicial é uma experiência simples, confiável e controlada pela própria comunidade. O servidor da aplicação continua responsável por identidade, autorização, histórico de mensagens e admissão nas salas de mídia. O encaminhamento de voz e tela usa um SFU. Uma arquitetura de mídia peer-to-peer não faz parte da primeira versão.

## Primeira versão de testes

- Há **uma comunidade**, no sentido de um servidor do Discord. Dentro dela, membros podem criar canais de texto e canais de voz. A estrutura deve permitir várias comunidades em uma versão futura, sem exigir que elas existam agora.
- O acesso é **somente por convite de um admin**. Não há cadastro público. O convidado faz um registro curto com login e senha antes de entrar na comunidade. E-mail e outros dados de perfil ficam para versões posteriores. Toda pessoa precisa se autenticar com login e senha.
- Existem apenas dois níveis de acesso no produto: **user** e **admin**. Todos os membros podem criar canais de texto e voz. Apenas admins podem convidar, expulsar temporariamente e banir users; admins não podem aplicar kick ou ban a outros admins. Pessoas autorizadas com acesso ao host terão um mecanismo administrativo separado para adicionar e remover admins. Um sistema detalhado de cargos e permissões por canal fica para versões futuras.
- Um **kick** impede a volta por pelo menos 30 minutos; o admin pode escolher um período maior. Um **ban** não expira automaticamente, mas um admin pode removê-lo. Uma queda de conexão não é kick: o cliente autorizado deve tentar voltar rapidamente e recuperar o estado perdido.
- Chat persistente, voz em tempo real e compartilhamento de tela devem funcionar juntos. Na primeira versão não há gravação ou transcodificação de mídia como requisito.
- O cliente nativo usa Rust. **Linux é a primeira prioridade**, incluindo validação separada de X11 e Wayland. **macOS vem em seguida** e **Windows depois**. O objetivo é suportar os três, com captura de tela, áudio e distribuição tratados conforme cada sistema operacional.
- Haverá um **cliente web para testes**. Ele não precisa ter paridade com o cliente nativo. A decisão de transformá-lo em produto permanente será tomada após os testes.
- A primeira versão de testes **não exige criptografia ponta a ponta (E2EE)**. O projeto deve reservar uma evolução concreta para E2EE de texto, voz e compartilhamento de tela **antes da versão final**. TLS e a criptografia de transporte da mídia não substituem E2EE.

## Escala e implantação inicial

O cenário de planejamento é **até 15 pessoas conectadas e dois compartilhamentos de tela simultâneos**. É um exemplo para dimensionar e testar o primeiro servidor, não um limite permanente nem uma garantia de desempenho. A implantação inicial usa um host Linux no Brasil, em VM de nuvem ou servidor doméstico com conectividade pública adequada. O mesmo host executa a aplicação, banco de dados, SFU e entrada TLS; os dados persistentes precisam de backup fora do host.

Usuários em outros países se conectam ao servidor brasileiro. O tráfego de mídia passa por ele. A experiência internacional depende de medições reais de latência, perda de pacotes e largura de banda. Quando o servidor estiver em uma residência, é necessário verificar IP público, CGNAT, portas e velocidade de upload.

## Caminho de desenvolvimento

1. **Validar riscos técnicos no Linux:** compilar o cliente e os serviços, demonstrar áudio bidirecional, captura e exibição de um quadro de tela, e medir a usabilidade e o desempenho da interface. X11 e Wayland exigem testes separados. O resultado decide se a escolha de UI e captura é viável.
2. **Entregar identidade e comunidade:** iniciar a primeira comunidade e seu primeiro admin com um procedimento seguro; admins emitem convites; convidados criam login e senha; membros autenticados criam canais. Aplicar as regras de user/admin desde o começo, inclusive em REST e WebSocket.
3. **Entregar chat confiável:** persistir mensagens em ordem por canal, confirmar envios e recuperar eventos perdidos após desconexão. Presença e indicação de digitação são estados temporários.
4. **Entregar voz e tela:** admitir apenas membros autorizados nas salas de voz, publicar microfone e tela e exibir os participantes e compartilhamentos. Validar NAT, CGNAT, TURN e reconexão com clientes em redes distintas.
5. **Validar a implantação brasileira:** medir o cenário de 15 pessoas e duas telas, segurança, backups e restauração. Criar o cliente web de testes para exercitar os mesmos contratos de servidor e mídia.
6. **Ampliar suporte e preparar a versão final:** validar macOS e depois Windows; definir e implementar E2EE com gestão de chaves e recuperação apropriadas; revisar as necessidades reais de capacidade, experiência e distribuição.

Essas etapas são objetivos, não um cronograma fechado. O teste da captura, da renderização de vídeo e da experiência de chat pode exigir mudança de ferramenta antes da implementação completa.

## Evolução prevista

- Várias comunidades por instalação, com convites e administração separados.
- Cargos e controles detalhados de acesso por comunidade e canal.
- Decisão sobre manter e ampliar o cliente web.
- Recursos como anexos, reações, mensagens diretas, busca e notificações conforme a prioridade observada nos testes.
- Escala para mais participantes ou múltiplos hosts somente após medição da demanda e do custo operacional.

## Decisões ainda abertas

A licença do repositório oficial precisa ser compatível com as dependências escolhidas, especialmente a ferramenta de UI. Ainda faltam o procedimento seguro de recuperação de conta sem e-mail, os detalhes do mecanismo de gestão de admins no host e o desenho de chaves e recuperação da E2EE. Essas decisões devem ser registradas antes de implementar as partes correspondentes.

Os detalhes de implantação, portas, fluxos de conectividade, riscos e controles de segurança estão em [architecture.md](./architecture.md). As decisões de engenharia estão no [índice de ADRs](./docs/adr/README.md), separadas por servidor e cliente. Questões adiadas e limites ainda não resolvidos estão em [PARKING_LOT.md](./PARKING_LOT.md).
