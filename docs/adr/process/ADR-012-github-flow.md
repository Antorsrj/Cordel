# ADR-012: Adotar GitHub Flow no lugar de Git Flow

**Status:** Proposed

**Delivery stage:** Repository process

**Date:** 2026-10-21

**Decider:** Project owner

## Context

O repositório oficial é `https://github.com/Antorsrj/Cordel.git`. Desde 2026-10-18 o código entra direto em `main`, sem proteção de branch. Não há `develop` nem branch de release.

O workflow `.github/workflows/phase0-linux.yml` dispara em `push` na `main` e por `workflow_dispatch`. Não há gatilho `pull_request`. O job formata, analisa com Clippy, testa e compila o cliente Phase 0 a partir de `client/phase0`. O filtro de caminhos também observa `client/media`, `client/state`, `protocol` e `docs/contracts/fixtures`. O restante do workspace, inclusive `server/identity`, fica de fora. Um commit pode chegar a `main` antes dessa checagem, e um pull request não a recebe.

Git Flow acrescentaria `develop` e branches de release de vida longa. O Cordel ainda tem uma linha de desenvolvimento. Esse desenho não cabe no estado atual do repositório.

## Proposed decision

Adotar GitHub Flow.

- `main` fica protegido e sempre verde. O push direto em `main` deixa de ser o caminho de integração.
- Cada tarefa usa um branch curto, criado a partir de `main`, com um destes prefixos: `feat/`, `fix/`, `docs/` ou `chore/`. O pull request liga esse branch à issue da tarefa. Issues continuam abertas por pessoas; um agente liga uma issue já existente e não cria outra para forçar o vínculo.
- Tudo entra por pull request. O CI aplicável precisa passar, e é obrigatória pelo menos uma review de Antorsrj, arleylambert ou do agente Reviewer.
- O merge é squash. O branch é apagado depois do merge.
- Uma versão é uma tag em `main`. Um branch de release só aparece se for preciso manter duas versões em paralelo.

Esta decisão não altera workflows, código nem configurações do GitHub.

## Implementation boundary

- **Owns:** o fluxo de branch, pull request, review, squash merge, apagamento do branch e tags de versão.
- **Handoff:** `AGENTS.md` e [WORKSTREAMS.md](../WORKSTREAMS.md) descrevem o mesmo fluxo para quem implementa. A proteção de branch e a ampliação do CI ficam com os mantenedores, numa mudança posterior.

## Options considered

| Option | Benefit | Cost or risk |
| --- | --- | --- |
| GitHub Flow **(proposed)** | `main` único, verde, com review antes de integrar | Depende de proteção de branch e de CI em pull request, ainda ausentes |
| Git Flow | Separa integração e release | `develop` e branches de release sem uma segunda linha de produto |
| Push direto em `main` **(prática desde 2026-10-18)** | Menos passos | Review não é obrigatória; o CI do Phase 0 só vê o commit depois que ele está em `main` |

## Consequences

- Pessoas e agentes passam a integrar por branch curto e pull request quando este ADR for aceito.
- Pendência, sem implementação aqui: o CI precisa passar a rodar em `pull_request` e cobrir mais do workspace do que o cliente Phase 0. `.github/workflows/phase0-linux.yml` permanece como está.
- Pendência, sem implementação aqui: os mantenedores configuram a proteção de branch de `main` no GitHub. Até lá, o repositório ainda aceita push direto; este ADR não muda essa configuração.
- Tags em `main` marcam versões. Branch de release fica reservado ao caso de duas versões em paralelo.
- Este ADR não entra no portão das 16 decisões obrigatórias do primeiro teste de produto.

## Acceptance and handoff

- O dono do projeto aceita GitHub Flow, os prefixos `feat/`, `fix/`, `docs/` e `chore/`, a review mínima (Antorsrj, arleylambert ou o agente Reviewer), o squash merge e as tags em `main`.
- Numa mudança posterior, os mantenedores registram a proteção de branch e o CI em `pull_request` com cobertura além de `client/phase0`.
- Enquanto o dono não aceitar o fluxo, este ADR permanece Proposed. As duas pendências acima são acompanhamento dos mantenedores, não parte desta mudança.

**Related:** [ADR-106](../client/ADR-106-license-distribution.md).

**Shared handoff:** [workstream map](../WORKSTREAMS.md); [agent instructions](../../../AGENTS.md).

**Complexity:** 2/5
