# ADR-0005 — Implementações de referência em Rust e TypeScript neste repositório, árbitro pelos vetores

**Status:** Aceito
**Data:** 2026-11-15
**Relacionados:** [ADR-0002](0002-noise-para-a-sessao-ponta-a-ponta.md), [ADR-0003](0003-cbor-com-schema-em-cddl.md), [ADR-0007](0007-toolchains.md), [vetores de teste](../especificacao/vetores-de-teste/README.md), [04 — Versionamento](../especificacao/04-versionamento.md)

## Contexto

O protocolo é falado em duas linguagens: Rust no Porecatu (host) e no Relay, TypeScript no Porecatu Remote (Expo). O dono escolheu Expo para o app, o que descarta compartilhar um núcleo Rust com o celular: haverá duas implementações do protocolo de qualquer forma.

O risco é o óbvio: duas implementações de um protocolo binário com criptografia, escritas em repositórios diferentes por quem está focado no próprio produto, divergem. A divergência aparece como "o celular não conecta" depois de uma subida de versão, e é cara de diagnosticar através de um relay cego.

O dono também decidiu por **quatro repositórios**, com a especificação num repositório próprio.

## Decisão

**As duas implementações de referência moram neste repositório — o crate Rust `porecatu-protocol` em `rust/` e o pacote TypeScript `@porecatu/protocol` em `ts/` — e as duas passam nos mesmos vetores de teste, que são o árbitro. O Porecatu e o Relay dependem do crate por versão exata; o Remote depende do pacote por versão exata.**

1. **Escopo das implementações**: envelope do relay (camada 2), handshake e registros Noise (camada 3), tipos e codificação das mensagens (camada 4). **Sem transporte e sem runtime**: nenhuma das duas abre WebSocket nem traz `tokio`; recebem e devolvem bytes. Quem consome liga ao próprio transporte.
2. **Estado puro e testável**: o handshake e o fluxo de registros são máquinas de estado alimentadas por bytes e pelo relógio passado de fora (mesma regra do Porecatu para estado com temporizador).
3. **Vetores** em JSON, em `docs/especificacao/vetores-de-teste/`, lidos pelos testes das duas. Mudança no fio entra com vetor novo no mesmo PR; o CI roda as duas implementações contra todos.
4. **Publicação**: crate e pacote com versão semântica própria, publicados juntos quando o fio muda. O Porecatu fixa `porecatu-protocol = "=x.y.z"`; o Relay também; o Remote fixa a versão exata no `package.json`.
5. **O Relay usa só a camada 2** do crate (módulo do envelope). Uma feature do crate desliga Noise e mensagens para o Relay não carregar o que não usa.

## Alternativas consideradas

- **Cada projeto implementa o protocolo no próprio repositório.** Sem dependência entre repositórios. Rejeitada: três implementações (host, relay, app), duas delas em Rust duplicando código, e nenhum lugar onde a conformidade cruzada é testada.
- **Só especificação aqui, sem código.** Repositório leve. Rejeitada: os vetores precisam de um gerador e de quem os rode; sem implementação de referência, "o vetor está certo?" não tem resposta.
- **Núcleo Rust compilado para o app** (via `uniffi` ou WebAssembly) em vez de TypeScript. Uma implementação só. Rejeitada por enquanto: o Expo gerenciado não carrega bibliotecas nativas sem build próprio, e WebAssembly no Hermes não é suportado de forma estável; o custo de build seria pago em todo o desenvolvimento do app. Pode ser revisto se a implementação TS se mostrar frágil.
- **Gerar o código TS a partir do Rust** (ex.: tipos por `ts-rs`). Reduziria divergência de tipos. Rejeitada: cobre só os tipos, não o handshake nem a codificação, que é onde divergência dói; os vetores cobrem tudo.

## Consequências

### Positivas

- Um lugar só onde a conformidade entre Rust e TS é provada, a cada PR.
- O Porecatu e o Relay recebem um crate pequeno, sem runtime, que encaixa nas regras de dependência do Porecatu (crate folha de rede é quem traz `tokio`, não este).
- Subir a versão do protocolo é uma mudança de versão exata nos consumidores, revisável.

### Negativas

- Duas implementações para manter neste repositório, em duas toolchains.
- Mudança no protocolo pede PR aqui, publicação e PRs nos consumidores: ciclo mais lento.
- A implementação TS de Noise é código de segurança próprio ([ADR-0002](0002-noise-para-a-sessao-ponta-a-ponta.md)).

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Vetores insuficientes deixarem passar divergência | Média | Alto | Casos obrigatórios por mensagem ([vetores](../especificacao/vetores-de-teste/README.md)); sessões completas gravadas; teste de ida e volta cruzado (Rust codifica, TS decodifica e vice-versa) |
| Consumidor fixar versão antiga e quebrar com o outro lado | Média | Médio | Negociação `[major, minor]` no `hello`; `incompatible` com mensagem clara no app |
| Publicação de crate/pacote dessincronizada | Baixa | Médio | Um workflow de release publica os dois a partir da mesma tag |
