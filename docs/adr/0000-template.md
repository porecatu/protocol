# ADR-0000 — Template e índice

## Índice de decisões

| # | Título | Status |
|---|--------|--------|
| [0001](0001-tres-partes-e-relay-cego.md) | Três partes e relay cego: o host é a fonte da verdade, o relay só roteia bytes cifrados | Aceito |
| [0002](0002-noise-para-a-sessao-ponta-a-ponta.md) | Noise para a sessão ponta a ponta: `XX` no pareamento, `IK` nas conexões seguintes | Aceito |
| [0003](0003-cbor-com-schema-em-cddl.md) | Mensagens em CBOR determinístico, com schema em CDDL e vetores de teste | Aceito · codificação no TS e `float32` revistos por ADR-0008 (**parcial**) |
| [0004](0004-websocket-sobre-tls.md) | Transporte por WebSocket sobre TLS, quadros binários, um envelope por quadro | Aceito |
| [0005](0005-implementacoes-de-referencia.md) | Implementações de referência em Rust e TypeScript neste repositório, árbitro pelos vetores | Aceito · envelope TS só do cliente revisto por ADR-0008 (**parcial**: os dois papéis) |
| [0006](0006-licenca-gpl.md) | GPL-3.0-or-later nos quatro projetos, com DCO | Aceito |
| [0007](0007-toolchains.md) | Toolchains pinadas: Rust stable e Node LTS com `pnpm` | Aceito |
| [0008](0008-especificacao-0-2.md) | Especificação 0.2: codificação estrita, sem ponto flutuante, vetores tipados e as lacunas achadas antes do código | Aceito · `span` e scrollback revistos por ADR-0009 (**parcial**) |
| [0009](0009-grafema-no-fio-e-linhas-descartadas.md) | Grafema composto marcado no fio e contador de linhas descartadas do histórico | Aceito |
| [0010](0010-aba-ativa-pelo-cliente.md) | Aba ativa pelo cliente: `activate-tab`, ativação como a do gesto local e janela do sistema intocada | Superseded by ADR-0011 (**parcial**: itens 1 a 4; as correções de texto 5 a 7 continuam) |
| [0011](0011-cliente-nao-muda-a-aba-ativa-e-view-tab.md) | O cliente não muda a aba ativa; `view-tab` informa a aba que o cliente vê | Aceito |
| [0012](0012-sessoes-nomeadas-pelo-cliente.md) | Sessões nomeadas pelo cliente: `list-sessions`, `sessions` e a ação `restore-session` | Aceito |

## Convenção

- Numeração sequencial de quatro dígitos, nunca reaproveitada.
- Nome do arquivo: `NNNN-titulo-em-kebab-case.md`.
- Status possíveis: `Proposto`, `Aceito`, `Rejeitado`, `Superseded by ADR-NNNN`.
- Decisão aceita não se edita. Para mudar, escreva um ADR novo com `Supersedes: ADR-NNNN` e marque o antigo. Correção de erro factual ou de clareza no texto é permitida; mudança de decisão, não.
- Revisão parcial de uma seção aceita entra como **blockquote de revisão** na própria seção, apontando o ADR novo, e a linha do índice acima ganha a nota "revisto por ADR-MMMM (**parcial**: …)".
- Decisão que muda o fio também muda a [especificação](../especificacao/00-visao-geral.md) no mesmo PR ([04 — Versionamento](../especificacao/04-versionamento.md)).
- Os números são **deste repositório**. ADRs do Porecatu, do Relay e do Remote são citados com o nome do projeto e URL absoluta (ex.: "ADR-0103 do Porecatu").

---

## Template

```markdown
# ADR-NNNN — Título

**Status:** Proposto | Aceito | Rejeitado | Superseded by ADR-MMMM
**Data:** AAAA-MM-DD
**Relacionados:** ADR-XXXX, seção da especificação

## Contexto

Qual é a força em jogo. O que torna esta decisão necessária agora, e o que
acontece se ela não for tomada. Requisitos e restrições concretos, não
generalidades.

## Decisão

O que foi decidido, em voz ativa e no presente. Uma frase, se possível.
Depois, os detalhes que a tornam acionável.

## Alternativas consideradas

Para cada alternativa séria: o que era, por que era plausível, e o motivo
específico de não ter sido escolhida. Alternativa sem motivo de rejeição
não é alternativa considerada, é enfeite.

## Consequências

### Positivas
### Negativas
### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
```
