# ADR-0011 — O cliente não muda a aba ativa; `view-tab` informa a aba que o cliente vê

**Status:** Aceito
**Data:** 2026-11-20
**Supersedes:** ADR-0010 (itens 1 a 4 da decisão; os itens 5 a 7 continuam valendo)
**Relacionados:** [ADR-0001](0001-tres-partes-e-relay-cego.md), [ADR-0008](0008-especificacao-0-2.md), [ADR-0009](0009-grafema-no-fio-e-linhas-descartadas.md), [ADR-0010](0010-aba-ativa-pelo-cliente.md), [03 — Mensagens](../especificacao/03-mensagens.md) §4.1, §4.4 e §7

## Contexto

O [ADR-0010](0010-aba-ativa-pelo-cliente.md) deixou o cliente mudar a aba ativa do desktop. Criou a ação `activate-tab` e fez as ações seguirem o gesto local por inteiro, inclusive a ativação de aba: `new-tab`, expandir grupo, `move-*-to-window` e `start-pane`. O app Remote passou a mandar `activate-tab` sozinho sempre que o usuário abre uma aba no terminal do celular, e o desktop troca a aba ativa da janela.

Na prática, foi uma decisão ruim, e o dono do produto a reverteu em 2026-11-20. Abrir uma aba no celular para **olhar** não é pedir que o desktop **mude**. A troca acontece debaixo de quem está no desktop, e o risco que a 0.3 evitava (as teclas de quem digita no desktop caírem em outra aba) passou a acontecer num gesto de leitura. As ações que ativam também deixavam o desktop num estado que ninguém nele pediu.

O que o desktop precisa saber é **qual aba o celular está vendo**, para mostrar isso a quem está diante dele, sem mudar nada. O protocolo não tem como dizer isso. A assinatura (`subscribe`, §5.1) é por painel e serve para receber a grade, e "assinado" não é "visto": o cliente pode assinar para outros fins, e a assinatura cai com `host-limit` enquanto o usuário continua na tela.

## Decisão

**O host não muda a aba ativa por causa do cliente. A ação `activate-tab` sai do protocolo. Uma mensagem nova, `view-tab`, informa ao host a aba que o cliente exibe, sem efeito sobre o workspace.**

1. **Volta a regra da 0.3.** O host **NÃO DEVE** mudar a aba ativa de uma janela por causa de uma mensagem do cliente.
   - `new-tab` cria a aba **sem ativar**.
   - `collapse-group { collapsed: false }` expande o grupo sem ativar nada.
   - `move-tab-to-window` e `move-group-to-window` chegam à janela de destino sem ativar nada.
   - `start-pane` inicia o painel sem ativar a aba dele.
   - A **escada de foco** continua quando uma ação **remove** a aba ativa de onde ela estava: fechar a aba ou o último painel dela, movê-la para outra janela, colapsar o grupo que a contém. Nesses casos, o desktop ativa a vizinha pela regra dele. O cliente não escolhe a aba: ela deixa de estar ali.
2. **`activate-tab` sai.** O código 13 de `action-body` fica **reservado** e nunca é reaproveitado. Um host 0.5 recusa `[13, …]` em `action-body` como valor fora do domínio (erro de protocolo, §1.1 de 03).
3. **Janela do sistema operacional, nunca.** Fica como no ADR-0010: o host **NÃO DEVE** levantar, desminimizar nem dar foco a uma janela do sistema por causa do cliente. O painel focado dentro da aba (`focused`) também não é mudado pelo cliente.
4. **Nova mensagem `view-tab`** (cliente → host), tipo **13**, no grupo do workspace:

   ```cddl
   view-tab = [13, { "tab": tab-id / null }]
   ```

   - **Significado.** Diz a aba que o cliente exibe agora. `null` significa nenhuma: a lista do workspace, outra tela ou nenhuma tela.
   - **Um estado por sessão.** Cada `view-tab` substitui o anterior. O estado começa `null` e o host o descarta no fim da sessão (`goodbye`, canal fechado, queda).
   - **Sem efeito no workspace.** `view-tab` não assina painel, não muda `active` nem `focused` e não gera `workspace`. O host usa o estado só para a sua própria apresentação, como o indicador de aba vista do Porecatu, e **NÃO DEVE** agir sobre o workspace por causa dele.
   - **Aba desconhecida.** O host responde `error { code: unknown-tab }` e o estado vira `null`. É a corrida normal de um cliente que pediu uma aba que acabou de fechar.
   - **A aba fecha.** O host limpa o estado sem mandar nada. O cliente já fica sabendo pelo `workspace`.
   - **A aba muda de janela ou de grupo.** O estado continua valendo, porque o `tab-id` é estável nessa mudança (§3 de 03).
   - **Obrigações do cliente.** O cliente **DEVE** mandar `view-tab { tab }` quando começa a exibir uma aba, inclusive de novo depois de reconectar, porque o estado é da sessão. Ele **DEVE** mandar `view-tab { tab: null }` quando para de exibi-la, e **PODE** mandar `null` ao ir para segundo plano. Trocar de painel dentro da mesma aba não pede `view-tab`.
   - **Só entre host e o cliente que mandou.** No v0, o host não repassa o estado a outros clientes.
5. **As correções de texto do ADR-0010** (itens 5 a 7: aba fixada, posição do `new-group`, casos de borda e ordem das checagens) **continuam valendo.** Elas não dependem da ativação.

O texto é o da especificação **0.5**. A versão de fio **continua `[0, 1]`**: nenhuma implementação publicada a congelou, e o precedente é o dos ADRs 0008, 0009 e 0010. A consequência, a mesma das versões anteriores, é que **a negociação não distingue a 0.4 da 0.5**: as duas anunciam `[0, 1]`, o handshake passa, e a incompatibilidade só aparece na primeira mensagem nova. Um host 0.4 recebe o tipo 13 como mensagem desconhecida e encerra a sessão com erro de protocolo (§1 de 03), e um cliente 0.4 que mandasse `activate-tab` a um host 0.5 cairia do mesmo jeito. Por isso as três implementações sobem juntas para a 0.5, como subiram para a 0.4.

## Alternativas consideradas

### Deduzir a aba vista das assinaturas

Não muda o fio: o host olharia os painéis que cada cliente assina e acharia a aba deles. Recusada por três motivos:

- "Assinado" vira "visto", o que prende o cliente a nunca assinar painel de outra aba (prévia, miniatura).
- O indicador some quando a assinatura cai por `host-limit` ou `pane-closed` com o usuário ainda na tela.
- Uma aba sem painel assinado agora (trocando de painel) desaparece por um instante.

### Reverter só o gesto de abrir a aba

Seria o app parar de mandar `activate-tab` ao abrir uma aba, mantendo a ação e a ativação nas outras ações. Recusada pelo dono: o problema é o celular mudar o desktop, não só esse gesto. Uma `activate-tab` que nenhuma UI usa só aumenta a área do fio.

### Manter `activate-tab` no protocolo, sem uso

Deixaria a porta aberta para um botão explícito no futuro. Recusada: uma ação sem uso não tem teste de ponta a ponta e induz implementações a usá-la. Se voltar, volta com um ADR e um caso de uso.

### `view-tab` repassado a todos os clientes

Mostraria num celular o que o outro está vendo. Recusada para o v0: nenhum cliente desenha isso hoje, e repassar o estado exige mensagem nova do host para o cliente sem ninguém que a consuma.

## Consequências

### Positivas

- Abrir uma aba no celular é um gesto de leitura: o desktop não muda.
- Quem está no desktop vê, pelo indicador do host, em que aba o celular está, o que reduz a surpresa de ver uma aba mudar de conteúdo sozinha.
- A regra de ativação fica uma só e curta: o cliente nunca ativa, e só a remoção da aba ativa a move.

### Negativas

- Ao voltar para o desktop, a aba em que se trabalhou pelo celular não está ativa. É preciso clicar nela, e o indicador ajuda a achá-la.
- Mais uma mudança de fio sem subir `[0, 1]`: host, Relay e Remote têm de andar juntos de novo.
- Expandir um grupo ou mover uma aba pelo celular deixa o desktop num estado diferente do que o gesto local deixaria. As ações remotas voltam a ter a exceção "sem ativar".

### Riscos e mitigação

| Risco | Probabilidade | Impacto | Mitigação |
|---|---|---|---|
| Cliente esquecer de mandar `view-tab null` e o host mostrar uma aba como vista indefinidamente | Média | Baixo | O estado morre com a sessão. O cliente manda `null` ao sair da tela, e os vetores de sessão cobrem a sequência |
| Host agir sobre o workspace por causa de `view-tab` (ativar, assinar) | Baixa | Médio | A regra é **NÃO DEVE**. O teste fica no host: `view-tab` não gera `workspace` |
| Implementação 0.4 conversar com uma 0.5 (o fio `[0, 1]` não as distingue) | Média | Médio | A sessão cai com erro de protocolo na primeira mensagem nova, sem efeito no workspace; host, Relay e Remote sobem juntos para o mesmo commit, e o congelamento de `[0, 1]` (P3) acaba com esse risco |
