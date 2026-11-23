# ADR-0013 — Nova aba solta: `new-tab` com `group: null`

**Status:** Aceito
**Data:** 2026-11-23
**Relacionados:** [ADR-0010](0010-aba-ativa-pelo-cliente.md), [ADR-0011](0011-cliente-nao-muda-a-aba-ativa-e-view-tab.md), [03 — Mensagens](../especificacao/03-mensagens.md) §7

## Contexto

Na especificação 0.6, `new-tab` sem `group` cria a aba "onde o `tab.new` do desktop criaria": no grupo da aba ativa da janela (ou solta, se a ativa for fixada). Com `group`, no fim desse grupo. Não existe jeito de pedir uma aba **solta** (sem grupo) quando a aba ativa está num grupo.

O desktop tem esse gesto: o "+" que fecha a trilha cria uma aba sem grupo no fim da barra, ignorando a aba ativa, separado do `tab.new` de propósito (ADR-0020 do Porecatu). O app mostra, no fim de cada janela, uma linha "Nova aba" que o usuário lê como "aba solta", e a manda sem `group`. Com a aba ativa no último grupo, a aba nasce nesse grupo e não solta, como o usuário pediu (relato de 2026-11-23).

## Decisão

**`group` em `new-tab` ganha um terceiro estado: `null` pede uma aba solta, no fim da trilha da janela.**

1. **Os três estados.** `group` ausente: onde o `tab.new` criaria (como na 0.6). `group` com um id: no fim do grupo (como na 0.6). `group: null`: aba **sem grupo**, no fim da trilha, sem olhar a aba ativa. É a mesma regra do "+" que fecha a trilha: se a trilha já termina em abas soltas, a nova entra entre elas; senão abre um trecho solto novo.
2. **Precedente no fio.** `move-tab-to-group` já usa `group: null` para "sai do grupo". Aqui `null` pede o lugar "fora de grupo".
3. **Erros.** `null` não valida grupo, então não produz `unknown-group`; `unknown-window`, `busy` e `unknown-profile` seguem a ordem de §7. Um `group` que não é id nem `null` (texto, por exemplo) é valor fora do domínio.
4. **Nada mais muda.** A aba criada continua sem ativar ([ADR-0011](0011-cliente-nao-muda-a-aba-ativa-e-view-tab.md)), `created` traz o `tab-id`, e o perfil e o `cwd` seguem as regras de antes.

O texto é o da especificação **0.7**. A versão de fio **continua `[0, 1]`**, pelo precedente dos ADRs 0008 a 0012: nenhuma implementação publicada a congelou. Um host 0.6 que receba `group: null` em `new-tab` recusa a mensagem como valor fora do domínio; enquanto `major = 0`, os dois lados exigem o mesmo `minor` e sobem juntos. Este ADR complementa os ADRs 0010 e 0011 (que definem o `new-tab` sem `group`); não substitui nenhum.

## Alternativas consideradas

### `group` ausente passa a significar aba solta

Não precisaria de campo novo. Recusada porque muda o significado de uma mensagem que já existe e tira do cliente o jeito de pedir o que o `tab.new` do desktop faz (a aba entra no grupo da aba ativa).

### Um campo novo (`loose: true`)

Evitaria dar um segundo sentido a `group`. Recusada: dois campos que se excluem pedem uma regra para a combinação inválida (`group` com `loose`), e `group: null` diz a mesma coisa com um estado só, como `move-tab-to-group` já diz.

### O cliente cria a aba e depois a tira do grupo (`move-tab-to-group` com `null`)

Não toca o fio. Recusada: são duas ações, a aba aparece um instante no grupo errado, e a falha da segunda deixa a aba no lugar errado sem o cliente ter como desfazer de forma atômica.

## Consequências

### Positivas

- O celular alcança o gesto do "+" do fim da trilha, e a linha "Nova aba" da janela passa a fazer o que o nome promete.
- Mudança aditiva e pequena: um estado novo num campo opcional, sem mensagem nem código novo.

### Negativas

- `group` passa a ter três estados, mais um caso para cada cliente tratar (ausente, `null`, id).
- Host e cliente 0.6 não falam com os 0.7 neste ponto.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Cliente confundir ausente com `null` e criar no lugar errado | Média | Baixo | Os dois têm vetor (`mensagens/action/minimo.json`, `new-tab-solta.json`) e teste de bytes nas duas implementações |
| Implementação 0.6 conversar com uma 0.7 (o fio `[0, 1]` não as distingue) | Média | Baixo | A sessão recusa `group: null` como valor fora do domínio; os três projetos sobem juntos |
