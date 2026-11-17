# Changelog

Mudanças da especificação e das implementações de referência. Formato inspirado no [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/). A versão de fio (`[major, minor]`) e a da especificação são explicadas em [04 — Versionamento](docs/especificacao/04-versionamento.md).

## [Não publicado]

### Especificação 0.4 (rascunho) — 2026-11-17

Versão de fio continua `[0, 1]`, ainda não congelada. Decisões no [ADR-0010](docs/adr/0010-aba-ativa-pelo-cliente.md).

- **Fio:** nova ação `activate-tab` (`action-body` código 13, `{ "tab": tab-id }`), que torna a aba a ativa na janela dela. Rust (`ActionBody::ActivateTab`), TypeScript (`activate-tab`), vetor `mensagens/action/activate-tab.json` e schema CDDL acompanham; um host 0.3 recusaria o código 13 como valor fora do domínio.
- **Regra:** o cliente pode mudar a aba ativa do desktop. As ações seguem o gesto local por inteiro, inclusive a ativação de aba (`new-tab`, `collapse-group` ao expandir, `move-*-to-window`, `start-pane`); o host **NÃO DEVE** levantar, desminimizar nem dar foco a janela do sistema por causa de ação remota; o painel focado dentro da aba e o tamanho do PTY continuam fora do alcance do cliente. Substitui a regra da 0.3 de que o host não muda o foco nem a aba ativa.
- **Correções de texto da §7 (o fio não muda):** `close-pane` e `close-tab` de aba fixada fecham a aba, sempre pedem confirmação (`running-process`, `processes` vazio se nada vive) e a desafixam, em vez de "reiniciar o painel"; `new-group` nasce na posição da primeira aba **na ordem da trilha**, e a ordem interna do grupo segue a da trilha; casos de borda escritos como regra (`move-tab-to-group` com grupo de outra janela, aba fixada ou `null` sem grupo; `move-*-to-window` para a mesma janela; `start-pane` fora de `not-started`/SSH dormente; `rename-tab` em fixada; `new-tab` cujo processo não sobe) e a ordem das checagens de toda ação (`unknown-*` → `busy` → regras da ação → confirmação).

### Correção da especificação 0.3 — 2026-11-16

- **Texto:** a regra do `start-pane` (§7 de 03) dizia que o cliente vê o painel em `connecting` enquanto uma pergunta do backend espera no desktop; passa a dizer `awaiting-desktop`, o estado que a 0.2 criou para esse caso (§4.1). O fio não muda.

### Implementações de referência, vetores e CI — 2026-11-16

Primeira implementação da especificação 0.3, versão de fio `[0, 1]`. Crate e pacote estão na versão `0.1.0` e ainda não foram publicados.

- **Crate Rust `porecatu-protocol`** (`rust/`): envelope do relay, sempre compilado (o Relay usa `default-features = false`); handshake Noise `XX`/`IK` sobre `snow` 0.10.0, registros com fragmentação e rekey, máquinas de estado `ClientSession` e `HostChannel`, `PairingToken` (feature `session`); as 26 mensagens de 03 e o `pair-request` (feature `message`); `host_id`, impressão digital, URI de pareamento e comparação em tempo constante. CBOR determinístico e estrito escrito à mão. Sans-IO, sem `tokio`, `unsafe_code = "deny"`, toolchain 1.98.0.
- **Pacote TypeScript `@porecatu/protocol`** (`ts/`): as mesmas camadas nos dois papéis, Noise sobre `@noble` 2.4.0, CBOR escrito à mão (sem `cbor-x`), ESM sem APIs do Node; entrada `@porecatu/protocol/testing` com chaves determinísticas, relógio manual, relay em memória e desktop falso.
- **Ferramenta de vetores** (`rust/tools/vectors`, não publicada): `generate`, `check` (contra o crate e o CDDL) e `schema`.
- **Vetores de teste:** 333 em `docs/especificacao/vetores-de-teste/` (identidade, envelope, mensagens, Noise com os do cacophony, registros, sessões). As sessões de `sessoes-ts/` são gravadas pelo TypeScript e reproduzidas pelo Rust: o teste cruzado roda nos dois sentidos.
- **Schema CDDL consolidado** `schema/porecatu-remote.cddl`, gerado dos blocos da especificação por `scripts/extract-cddl.py`; o `verify-docs.py` reprova o arquivo fora de sincronia.
- **CI** (`.github/workflows/ci.yml`): crate em Linux, Windows e macOS (fmt, clippy, testes, build sem features padrão, ausência de `tokio`); `cargo deny`; pacote TS (typecheck, lint, testes, build); `vectors check`; verificação da documentação; DCO nos PRs.

### Especificação 0.3 (rascunho) — 2026-11-16

Versão de fio continua `[0, 1]`, ainda não congelada. Decisões no [ADR-0009](docs/adr/0009-grafema-no-fio-e-linhas-descartadas.md).

- **Fio:** o `text` do `span` passa a ser `tstr / [tstr]` — array de um elemento marca um único grafema de vários code points numa célula; `grid-snapshot`, `grid-diff` e `scrollback-page` ganham `dropped` (linhas já descartadas do topo do histórico), que dá a cada linha de histórico um índice absoluto estável.

### Especificação 0.2 (rascunho) — 2026-11-15

Versão de fio continua `[0, 1]`, ainda não congelada. Decisões no [ADR-0008](docs/adr/0008-especificacao-0-2.md).

- **Fio:** `ratio` passa de `float32` a inteiro `1..65535`; `welcome` ganha `host_instance`; `error` ganha `pane` opcional; `seq` monotônico por assinatura; código de erro `last-tab`; estado de painel `awaiting-desktop`; `title` na janela; consulta de presença `GET /v0/presence/<host_id>`; código de fechamento `4005`.
- **Regras:** codificação e decodificação estritas (§1.1 de 03); forma exata da URI de pareamento; falha de handshake e DH nulo; ciclo e erros do envelope no relay; resultado de ação só em `action-result`.
- **Vetores:** JSON tipado, formato de sessão, tabela de casos por categoria.
- **Texto:** regra CDDL `text` renomeada para `text-input`; o schema consolidado é gerado dos blocos de 01, 02 e 03.

### Especificação 0.1 (rascunho) — 2026-11-15

- Primeira redação: transporte e envelope do relay (`/v0/`), chaves e `host_id`, pareamento por QR com Noise `XX`, conexões com Noise `IK`, registros com fragmentação e rekey, mensagens de aplicação em CBOR com schema em CDDL (workspace, perfis, tema, grade com snapshot/diff e controle de fluxo, scrollback, input semântico, ações com confirmação, erros tipados), modelo de ameaça e formato dos vetores de teste.
- Versão de fio proposta: `[0, 1]`, ainda não congelada.
- ADR-0001 a ADR-0007.
