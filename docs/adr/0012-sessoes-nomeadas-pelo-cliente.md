# ADR-0012 — Sessões nomeadas pelo cliente: `list-sessions`, `sessions` e a ação `restore-session`

**Status:** Aceito
**Data:** 2026-11-20
**Relacionados:** [ADR-0001](0001-tres-partes-e-relay-cego.md), [ADR-0008](0008-especificacao-0-2.md), [ADR-0011](0011-cliente-nao-muda-a-aba-ativa-e-view-tab.md), [03 — Mensagens](../especificacao/03-mensagens.md) §3, §4.5, §7 e §9, [PRD-000](../prd/prd-000-visao-de-produto.md)

## Contexto

O Porecatu grava **sessões nomeadas**: a disposição de uma janela (grupos, abas, painéis, diretórios), salva com um nome e restaurada quando o usuário pede ([PRD-014](https://github.com/porecatu/terminal/blob/main/docs/prd/prd-014-sessoes-nomeadas.md) do Porecatu). Uma chave do desktop, `[session] restore_named_in_current_window`, decide o destino da restauração. Desligada, que é o padrão, abre uma janela nova. Ligada, acrescenta os grupos e as abas da sessão ao fim da janela de onde o gesto partiu ([PRD-023](https://github.com/porecatu/terminal/blob/main/docs/prd/prd-023-restaurar-sessao-nomeada-na-janela-atual.md) do Porecatu).

A especificação deixou sessões nomeadas fora do v0 de propósito (§7 de 03, PRD-000). Em 2026-11-20, o dono do produto pediu que o celular possa **escolher uma sessão salva e restaurá-la**: quem está longe do computador quer montar o ambiente do projeto X sem esperar chegar à mesa.

O pedido esbarra em três regras que já existem:

1. **O cliente não muda a aba ativa nem a janela do sistema** ([ADR-0011](0011-cliente-nao-muda-a-aba-ativa-e-view-tab.md)). A restauração do desktop faz as duas coisas. Na janela nova, a janela do sistema ganha o foco. No acréscimo, a aba que era ativa na sessão passa a ser a ativa da janela, e um terminal parado que era a única aba da janela é fechado.
2. **No celular não existe "janela atual".** A chave do desktop fala da janela de onde o gesto partiu, e o gesto remoto não parte de janela nenhuma.
3. **Os ids do protocolo são números do host** (§3 de 03). O cliente nunca vê nada interno, e um arquivo de sessão é identificado no host por um caminho no disco.

## Decisão

**O cliente pede a lista das sessões salvas e restaura uma delas por uma ação. O destino segue a chave do desktop, a restauração nunca ativa aba nem janela do sistema e nunca fecha nada. Salvar e excluir continuam só no desktop.**

1. **Pedir a lista (`list-sessions`, tipo 14, cliente → host).** O corpo é `{ "id": request-id }`. O host lê as sessões salvas **só sob pedido**, como faz o popover do desktop, e nunca empurra a lista. Por isso não há mensagem a cada mudança no disco.
2. **A resposta (`sessions`, tipo 15, host → cliente).** O corpo tem `id`, `mode` e `list`:
   - **`id`** casa a resposta com o pedido.
   - **`mode`** é o valor atual da chave do desktop: `new-window` (0) ou `append` (1). O cliente precisa dele para saber se pergunta a janela.
   - **`list`** traz as sessões **na ordem do desktop**, a salva mais recentemente primeiro. Cada entrada tem `session` (id), `name` (o nome que o usuário deu, repassado como está) e `readable`.
   - Uma sessão que o desktop não consegue ler (arquivo corrompido, schema mais novo) vem com `readable: false` e, em `name`, o rótulo que o desktop mostra para ela, para o cliente listá-la esmaecida como o desktop lista.
3. **Um id novo: `session-id`** (`uint`, §3 de 03).
   - O escopo é o processo do host, como os outros ids.
   - O mesmo arquivo recebe o mesmo id durante a vida do processo, e um id nunca é reaproveitado.
   - Como todo id, só vale entre sessões com o mesmo `host_instance`.
   - O cliente nunca vê caminho nem nome de arquivo.
4. **Restaurar é uma ação: `restore-session`**, código **14** de `action-body`. O 13 continua reservado. O corpo é `{ "session": session-id, ? "window": window-id }`, e a resposta vem em `action-result`, como em toda ação.
   - **O destino é a chave do desktop no momento da ação**, não o `mode` que a lista trouxe. A chave vale na hora no desktop e pode ter mudado entre a lista e o pedido.
   - **`new-window`.** O host abre uma janela nova com a disposição da sessão e ignora `window`. A janela usa o tamanho salvo e a posição em cascata a partir da janela de foco mais recente, como a restauração do desktop faz a partir da janela de origem.
   - A janela nova **nasce sem foco**: o host **NÃO DEVE** levantá-la, ativá-la nem pô-la à frente das outras na ordem de foco. Com isso, a regra 2 de "Aba ativa e janela do sistema" (§7) continua valendo sem exceção.
   - **`append`.** O host acrescenta os grupos e as abas da sessão **ao fim da trilha** da janela `window`, como o desktop faz com a chave ligada. Sem `window`, responde `invalid-argument`.
   - **Nenhuma ativação e nenhum fechamento**, nos dois destinos. A aba ativa de toda janela que já existia continua a mesma: o host **NÃO DEVE** ativar a aba que era ativa na sessão. Também **NÃO DEVE** fechar aba nenhuma, nem o terminal parado que o gesto local fecharia. É a regra 1 de "Aba ativa e janela do sistema" (§7), sem exceção. Na janela nova, a aba ativa é a da sessão, porque é a primeira aba ativa de uma janela que não existia.
   - **O que a restauração faz é o que a do desktop faz:** diretórios com o mesmo fallback, árvore de painéis, início adiado de abas e arquivo de projeto só em diretório autorizado. Tema, zoom e abas fixadas gravados não são aplicados.
   - **Sucesso não traz `created`.** A janela ou as abas novas chegam pelo `workspace` seguinte.
5. **Erros e ordem das checagens.** A ordem continua a de §7: ids desconhecidos, depois `busy`, depois as regras da ação.
   - Dois códigos novos: **`unknown-session` (17)** e **`session-unreadable` (18)**.
   - A sessão que sumiu do disco entre a lista e o pedido é `unknown-session`.
   - O arquivo que o host não consegue ler, mesmo listado como legível, é `session-unreadable`. O host o trata como o desktop trata.
   - Ordem: `unknown-session` → `unknown-window` (se `window` veio) → `busy` → `invalid-argument` → `session-unreadable`. O arquivo só é lido depois de todas as outras checagens.
   - **`busy`.** No acréscimo, vale quando a janela alvo está num estado que impede a ação. Na janela nova, quando o desktop inteiro está (um arraste entre janelas em curso).
6. **Fora do v0, de propósito.** Salvar a janela como sessão nomeada e excluir uma sessão salva. A troca é de texto: o item "sessões nomeadas" da lista de §7 vira "salvar e excluir sessões nomeadas". "Abrir janela nova" vira "abrir janela vazia", porque a restauração passa a abrir janela.

O texto é o da especificação **0.6**. A versão de fio **continua `[0, 1]`**, pelo precedente dos ADRs 0008 a 0011: nenhuma implementação publicada a congelou. A consequência é a mesma das versões anteriores: a negociação não distingue a 0.5 da 0.6. Um host 0.5 que receba `list-sessions` (tipo desconhecido) ou `[14, …]` em `action-body` (fora do domínio) encerra a sessão com erro de protocolo. Por isso host, Relay e Remote sobem juntos para o commit da 0.6.

## Alternativas consideradas

### Empurrar a lista no começo da sessão, como `profiles`

O cliente teria a lista sem pedir, e uma mensagem a cada mudança a manteria em dia. Recusada porque o desktop lê o diretório das sessões **só sob pedido** (RF-14.18 do Porecatu). Empurrar exigiria ler no começo de toda sessão e vigiar o disco em segundo plano, um custo para uma folha que o usuário abre raramente.

### Usar o nome do arquivo (ou o nome da sessão) como id

Evitaria a tabela de ids no host. Recusada por duas razões. O cliente passaria a mandar um texto que o host transforma em caminho, e um `../` mal tratado abriria um arquivo fora do diretório. E o nome não é chave: o desktop compara nomes sem distinguir caixa, e dois nomes diferentes podem gerar o mesmo nome de arquivo. Um número do host, como os outros ids, não tem nenhum dos dois problemas.

### O cliente escolhe o destino, ignorando a chave

A folha ofereceria "Janela nova" ou uma das janelas, sempre. Recusada pelo dono do produto: a chave é a preferência de quem usa o desktop, e restaurar pelo celular deve dar o mesmo resultado que restaurar no desktop. A diferença fica só onde não há equivalente, a "janela atual", que o cliente informa por `window`.

### Restaurar como o desktop, ativando a aba e fechando o terminal parado

Seguiria o gesto local por inteiro. Recusada: é exatamente o que o [ADR-0011](0011-cliente-nao-muda-a-aba-ativa-e-view-tab.md) desfez. Mudar a aba ativa ou fechar uma aba debaixo de quem está no desktop é surpresa, e as teclas de quem digita podem cair noutro lugar.

### Pedir a confirmação do cliente antes de restaurar

Como o fechamento com processo vivo (`confirm-request`). Recusada: restaurar não destrói nada (o acréscimo não fecha aba e a janela nova não toca nas outras), e a ação já é o gesto de confirmação na folha do cliente.

## Consequências

### Positivas

- O celular monta o ambiente de um projeto sem que o usuário esteja no computador, com o mesmo resultado da restauração do desktop.
- As regras do ADR-0011 continuam sem exceção: nenhuma aba ativa muda, nenhuma janela do sistema ganha foco e nada fecha.
- O id de sessão segue o modelo dos outros: número do host, sem caminho no fio.

### Negativas

- O host precisa de uma janela que nasce sem foco. Nem todo sistema gráfico deixa o programa decidir isso: no X11 e no Wayland, quem decide é o gerenciador de janelas (veja "Riscos").
- Mais uma mudança de fio sem subir `[0, 1]`: host, Relay e Remote andam juntos de novo.
- A restauração remota diverge da local em duas coisas, a aba ativa e o terminal parado, e o usuário pode estranhar a diferença.
- A lista do cliente pode envelhecer. Uma sessão salva ou excluída no desktop depois da lista só aparece no próximo pedido, e restaurar a que sumiu dá `unknown-session`.

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| A janela nova roubar o foco num sistema em que o host não controla a ativação (X11, Wayland) | Alta nesses sistemas | Médio | A regra é **NÃO DEVE**. O host usa o que o sistema oferece, registra a limitação como dívida de verificação, e o cliente não a contorna |
| O cliente guardar `session-id` entre sessões e restaurar outra sessão depois que o host reiniciou | Baixa | Médio | Os ids só valem com o mesmo `host_instance` (§3 de 03). O cliente pede a lista ao abrir a folha |
| A chave do desktop mudar entre a lista e a ação | Baixa | Baixo | O host decide pela chave no momento da ação. No acréscimo sem `window`, `invalid-argument`, e o cliente pede a lista de novo |
| Implementação 0.5 conversar com uma 0.6 (o fio `[0, 1]` não as distingue) | Média | Médio | A sessão cai com erro de protocolo na primeira mensagem nova, sem efeito no workspace. Os três projetos sobem juntos, e o congelamento de `[0, 1]` (P3) acaba com o risco |
