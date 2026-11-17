# ADR-0010 — Aba ativa pelo cliente: `activate-tab`, ativação como a do gesto local e janela do sistema intocada

**Status:** Aceito
**Data:** 2026-11-17
**Relacionados:** [ADR-0001](0001-tres-partes-e-relay-cego.md), [ADR-0008](0008-especificacao-0-2.md), [ADR-0009](0009-grafema-no-fio-e-linhas-descartadas.md), [03 — Mensagens](../especificacao/03-mensagens.md) §4 e §7

## Contexto

A especificação até a 0.3 dizia, na §7 de 03, que o host **não muda o foco nem a aba ativa** do desktop por causa de uma ação remota, e na §4.1 que "o cliente nunca muda o foco do desktop". A razão era um risco concreto: quem está no desktop digita na aba ativa; se o celular a trocasse no meio, as teclas dessa pessoa cairiam na aba errada. Daí a regra de que uma aba criada pelo celular nascesse sem roubar a ativação.

O dono do produto decidiu em 2026-11-17 que o celular **pode** mudar a aba ativa do desktop. O uso típico é uma pessoa só, longe da mesa, operando a sessão pelo celular; nesse uso a regra antiga só atrapalhava (criar uma aba e ela não ficar ativa no desktop, mover uma aba para outra janela e ela não ficar à vista, expandir um grupo sem que o desktop faça o que o gesto local faz). O dono aceita o risco de digitar na aba errada quando as duas pontas são usadas ao mesmo tempo.

Nenhum ADR anterior fixava essa regra: ela estava só no texto da especificação (§4.1 e §7 de 03) e no [PRD-000](../prd/prd-000-visao-de-produto.md), que a lista entre as exclusões do v0. Este ADR a revê e o texto muda no mesmo PR.

Durante a implementação do host, outras lacunas da §7 apareceram: duas frases da 0.3 descreviam um comportamento que o desktop não tem, e vários casos de borda não estavam escritos. Estão listados abaixo como correções de texto; elas **não mudam o fio**.

## Decisão

**O cliente pode mudar a aba ativa de uma janela; as ações seguem o gesto local por inteiro, inclusive a ativação de aba que ele faz; a janela do sistema operacional nunca muda por causa de uma ação remota.**

1. **Ações seguem o gesto local por inteiro, inclusive a ativação de aba.** `new-tab` deixa a aba criada ativa na janela dela (como o `tab.new`); expandir um grupo (`collapse-group { collapsed: false }`) ativa o que o desktop ativa ao expandir; `move-tab-to-window` e `move-group-to-window` deixam a aba ou o grupo movido ativo na janela de destino, como o arraste e o "Mover para janela…"; colapsar o grupo da aba ativa move a aba ativa pela regra do desktop; `start-pane` deixa a aba do painel ativa, porque no desktop a aba não iniciada sobe quando é ativada.
2. **Nova ação `activate-tab`**, `[13, { "tab": tab-id }]`: torna a aba a ativa **na janela dela**. Aba desconhecida: `unknown-tab`. Aba já ativa: sucesso, sem efeito. O resto segue a regra do desktop para ativar uma aba (aba num grupo colapsado: o desktop expande o grupo; aba não iniciada: o desktop a inicia), sem a especificação detalhar a implementação do Porecatu.
3. **Janela do sistema operacional, nunca.** O host **NÃO DEVE** levantar, desminimizar nem dar foco a uma janela do sistema por causa de uma ação remota — nem em `activate-tab`, nem na janela de destino de `move-*-to-window`. Só a aba ativa *dentro* da janela muda. O painel focado dentro da aba (`focused`) não é mudado pelo cliente; não há ação para isso.
4. **Continua valendo:** a confirmação vai ao cliente e nunca abre diálogo no desktop; o cliente nunca muda o tamanho do PTY; `key` vai ao terminal, nunca à tabela de ações do desktop.

Texto da especificação 0.4; a versão de fio **continua `[0, 1]`**: ela ainda não foi congelada por nenhuma implementação publicada, e o precedente é o [ADR-0008](0008-especificacao-0-2.md) e o [ADR-0009](0009-grafema-no-fio-e-linhas-descartadas.md). A ação nova é um valor a mais em `action-body`; um host 0.3 a recusaria como valor fora do domínio, e `major = 0` já exige `minor` igual ([04](../especificacao/04-versionamento.md)).

### Correções de texto da §7 (o fio não muda)

5. **Fixada.** `close-pane` do último painel de uma aba fixada **fecha a aba**, como no desktop (a 0.3 dizia que a fixada ficava com o painel reiniciado, regra que o desktop não tem). Fechar uma aba fixada, por `close-pane` ou por `close-tab`, sempre pede `confirm-request` e a desafixa; uma fixada sem nada vivo recebe `kind = running-process` com `processes` vazio.
6. **`new-group`.** O grupo nasce na posição da **primeira das abas na ordem da trilha**, e a ordem interna do grupo segue a da trilha, não a da lista (a 0.3 dizia "a primeira da lista").
7. **Casos que a 0.3 não definia** (o host já responde assim):
   - `move-tab-to-group` com grupo de **outra janela**: `unknown-group`; com aba fixada: `invalid-argument`; `group: null` com a aba já fora de grupo: sucesso, sem efeito.
   - `move-tab-to-window` e `move-group-to-window` para a **mesma janela**: `invalid-argument`; mover a última aba de uma janela é permitido (a janela que esvazia fecha); aba fixada vai fixada.
   - `start-pane` em painel que não está `not-started` nem é SSH dormente: `invalid-argument`.
   - `rename-tab` em aba fixada é aceito.
   - `new-tab` cujo processo não sobe: `invalid-argument`.
   - Ordem das checagens de toda ação: ids desconhecidos (`unknown-*`, na ordem dos campos), depois `busy`, depois as regras da ação (`invalid-argument`, `last-tab`), depois a confirmação.

## Alternativas consideradas

### Manter a proibição da 0.3

Era a opção mais segura para quem usa as duas pontas ao mesmo tempo. Rejeitada pelo dono: o caso de uso dominante é uma pessoa só, e a proibição fazia ações comuns (criar uma aba, mover uma aba entre janelas) deixarem o desktop num estado diferente do que o gesto local deixa.

### Só `activate-tab`, sem ativação nas outras ações

Manteria `new-tab` e `move-*-to-window` sem ativar. Rejeitada: duas regras de ativação (uma para o gesto local, outra para o remoto) divergem a cada mudança no desktop, e "seguir o gesto local por inteiro" é a regra que não precisa ser reescrita junto com ele.

### Também levantar a janela do sistema

Faria `activate-tab` mostrar a janela da aba. Rejeitada: roubar o foco do sistema operacional interromperia o que o usuário faz em outro programa, um efeito bem maior que trocar a aba dentro de uma janela do Porecatu, e levantar uma janela minimizada é visível a quem estiver no desktop. Só a aba ativa dentro da janela muda.

### Ação para mudar o painel focado

Rejeitada para o v0: o cliente assina e digita por `pane-id`, não depende do `focused` do desktop, e uma ação a mais ampliaria o risco de digitar no painel errado sem ganho.

## Consequências

### Positivas

- O celular opera o desktop como o gesto local: criar, mover e expandir deixam a aba à vista, e navegar entre abas pelo celular é possível.
- Uma regra só ("o gesto local, por inteiro") em vez de uma lista de exceções.
- A §7 passa a escrever os casos de borda que o host já tratava, para as implementações não divergirem.

### Negativas

- Quem usa o desktop ao mesmo tempo pode ver a aba ativa mudar sob as mãos e digitar na aba errada.
- Um host 0.3 e um cliente 0.4 não conversam (já era o caso com `major = 0`).

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Digitar na aba errada porque o celular trocou a aba ativa | Baixa (uso típico de uma pessoa só) | Médio | Risco aceito pelo dono; o host nunca mexe na janela do sistema, o que limita o efeito à aba ativa dentro da janela; o painel focado e o tamanho do PTY não mudam |
| Host levantar a janela do SO por engano em `activate-tab` ou no destino de `move-*-to-window` | Média | Médio | A regra é NÃO DEVE; o teste fica no host (verificação ao vivo no Windows) |
| Implementações divergirem nos casos de borda do item 7 | Baixa | Baixo | Estão escritos na §7; as respostas viram teste no host |
