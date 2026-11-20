# 03 — Mensagens

**Versão da especificação:** 0.6 (rascunho)

Este documento cobre a camada 4: as mensagens de aplicação que host e cliente trocam dentro da sessão Noise. A escolha de CBOR e CDDL está no [ADR-0003](../adr/0003-cbor-com-schema-em-cddl.md); as regras estritas de codificação e decodificação desta versão, no [ADR-0008](../adr/0008-especificacao-0-2.md).

## 1. Convenções

- Cada mensagem é **um item CBOR** (RFC 8949), remontado dos registros ([02](02-handshake-e-pareamento.md) §5).
- Toda mensagem é um array `[tipo, corpo]`, com `tipo` inteiro (tabela do §10) e `corpo` um mapa de chaves de texto.
- **Chave desconhecida num mapa é ignorada.** É o que permite acrescentar campo numa versão menor ([04](04-versionamento.md)). Chave obrigatória ausente é erro de protocolo. No schema consolidado (anexo A), todo mapa de corpo termina em `* tstr => any`.
- **Tipo de mensagem desconhecido é erro de protocolo.** Mensagem nova numa versão menor só é mandada a quem anunciou essa versão menor no `hello`/`welcome`. Mensagem de tipo conhecido **no sentido errado** (ex.: `welcome` vindo do cliente) também é erro de protocolo.
- Erro de protocolo (CBOR inválido ou não canônico, campo obrigatório ausente, tipo desconhecido, valor fora do domínio): quem detecta manda `error { code: protocol }` e fecha a sessão.

### 1.1 Codificação e decodificação estritas

Duas implementações em linguagens diferentes precisam concordar **byte a byte**. Por isso:

1. **Quem manda DEVE** usar a codificação determinística da RFC 8949 §4.2.1: inteiros e comprimentos na forma mais curta, comprimentos definidos, chaves de mapa ordenadas pela ordem lexicográfica dos seus bytes codificados.
2. **Quem recebe DEVE recusar** como erro de protocolo:
   - qualquer item que não esteja na forma determinística (inteiro ou comprimento não mínimo, chaves fora de ordem, comprimento indefinido);
   - chave de mapa duplicada;
   - bytes sobrando depois do item;
   - **tags** CBOR de qualquer tipo e **números de ponto flutuante**: nenhuma mensagem desta versão os usa;
   - valores simples além de `false`, `true` e `null`;
   - inteiro fora de `-(2^53 − 1) .. 2^53 − 1` (o maior inteiro exato em JavaScript);
   - aninhamento de arrays e mapas além de **32** níveis;
   - array posicional (envelopes, `span`, `style`, `line`, `color`, `action-body`, pares `[row, line]`, versões) com número de elementos diferente do definido.
3. Chaves desconhecidas são ignoradas **depois** da conferência acima: um mapa com chave desconhecida ainda tem de estar em forma canônica.

A forma recomendada de conferir o item 2 é decodificar para uma árvore genérica, recodificar deterministicamente e comparar os bytes.
- **Nenhum texto para humano é gerado pelo protocolo.** Motivos e erros são códigos; títulos, nomes e diretórios são dados do usuário, repassados como estão.
- Cores são `rgb = uint .le 0xFFFFFF` (`0xRRGGBB`).
- O schema completo, num bloco só, está no [anexo A](#anexo-a--schema-cddl-completo).

## 2. Início da sessão

Logo depois do handshake `IK` (ou do `pair-accepted`, no pareamento), o cliente manda `hello`:

```cddl
hello = [0, {
  "version":     [major: uint, minor: uint],   ; o que o cliente fala
  "app":         tstr,                          ; ex.: "porecatu-remote"
  "app_version": tstr,                          ; semver do app
}]
```

O host responde com `welcome` ou `incompatible`:

```cddl
welcome = [1, {
  "version":          [major: uint, minor: uint],  ; major igual; minor = menor dos dois
  "host_name":        tstr,                        ; nome do host para exibição
  "porecatu_version": tstr,                        ; semver do Porecatu
  "max_subscriptions": uint,                       ; >= 1; o host atual usa 4
  "host_instance":    bstr .size 16,               ; aleatório, novo a cada processo do host (§3)
}]

incompatible = [2, {
  "version": [major: uint, minor: uint],           ; o que o host fala
}]
```

O host manda `incompatible` quando o `major` difere ou, **enquanto `major = 0`**, quando o `minor` difere ([04](04-versionamento.md)): no período de rascunho não há negociação de `minor`, e "o menor dos dois" só vale a partir de `major = 1`. Depois de `incompatible` o host fecha o canal. Depois de `welcome`, o host manda **na ordem** `theme`, `profiles` e `workspace` (§4), e a sessão está pronta.

Qualquer lado pode encerrar com `goodbye` antes de fechar o canal:

```cddl
goodbye = [3, { "reason": goodbye-reason }]
goodbye-reason = &(
  normal:          0,
  remote-disabled: 1,   ; o usuário desligou o acesso remoto no host
  revoked:         2,   ; este aparelho foi removido da lista de pareados
  host-quitting:   3,   ; o Porecatu está fechando
)
```

## 3. Identificadores

| Id | Escopo | Regra |
|---|---|---|
| `window-id` | processo do host | Único durante a vida do processo; nunca reaproveitado |
| `group-id` | processo do host | Idem. **Estável quando o grupo muda de janela** |
| `tab-id` | processo do host | Idem. **Estável quando a aba muda de janela ou de grupo** |
| `pane-id` | processo do host | Idem. Estável enquanto o painel existir |
| `profile-id` | configuração do host | O identificador de perfil do Porecatu (texto), como ele aparece na sessão gravada |
| `session-id` | processo do host | Uma sessão nomeada salva no host (§4.5). Único durante a vida do processo, nunca reaproveitado; **o mesmo arquivo de sessão recebe o mesmo id** enquanto o processo vive |
| `request-id` | sessão | Escolhido pelo cliente, único entre os pedidos pendentes da sessão |

Os ids do protocolo **não são** os ids internos do Porecatu (que são por janela e mudam quando uma aba muda de janela). O host mantém o mapeamento; o cliente nunca vê id interno, nem caminho ou nome de arquivo. Reiniciar o Porecatu reinicia todos os ids.

**`host_instance`** (no `welcome`) é um valor aleatório de 16 bytes que o host gera ao iniciar o processo. Os ids só são comparáveis entre duas sessões **com o mesmo `host_instance`**: um cliente que reconecta e recebe outro valor **NÃO DEVE** reaproveitar nenhum id guardado (aba aberta, zoom por aba, rascunho), porque o mesmo número pode ser outra aba — e digitar na aba errada é o pior erro deste protocolo.

```cddl
window-id  = uint
group-id   = uint
tab-id     = uint
pane-id    = uint
profile-id = tstr
session-id = uint
request-id = uint
```

## 4. Estado do workspace

### 4.1 `workspace`

O host manda a **árvore inteira** a cada mudança estrutural ou de rótulo, coalescida em no máximo **4 mensagens por segundo**. Uma árvore de cinquenta abas cabe em poucos KB; mandar inteira poupa o cliente de aplicar deltas e o protocolo de numerá-los.

```cddl
workspace = [10, {
  "rev":     uint,                 ; cresce a cada envio na sessão
  "windows": [* window],           ; na ordem de foco do host, mais recente primeiro
}]

window = {
  "id":     window-id,
  "title":  tstr,                  ; título da aba ativa da janela, para o cliente identificar a janela
  "pinned": [* tab],               ; abas fixadas, na ordem da zona
  "items":  [* item],              ; a trilha: abas soltas e grupos, na ordem visual
  "active": tab-id / null,         ; aba ativa da janela no desktop
}

item  = tab / group

group = {
  "group":     group-id,           ; a presença desta chave distingue grupo de aba
  "name":      tstr,
  "color":     uint,               ; índice em theme.group_colors
  "collapsed": bool,
  "tabs":      [+ tab],
}

tab = {
  "tab":     tab-id,
  "title":   tstr,                 ; o título que o desktop mostra, já com a precedência aplicada
  "custom":  bool,                 ; true se o título foi definido pelo usuário
  "profile": profile-id / null,
  "remote":  tstr / null,          ; "usuário@host" em aba SSH; null nas locais
  "activity": bool,                ; indicador de atividade em segundo plano
  "bell":    bool,                 ; indicador de campainha
  "layout":  pane-node,
  "focused": pane-id,              ; painel focado da aba
}

pane-node = pane / split

split = {
  "dir":   &(side-by-side: 0, stacked: 1),
  "ratio": 1..65535,               ; fração do primeiro filho = ratio / 65536 (inteiro: sem ponto flutuante no fio)
  "first": pane-node,
  "second": pane-node,
}

pane = {
  "pane":  pane-id,
  "title": tstr,
  "cwd":   tstr / null,            ; diretório local; null se desconhecido ou se o painel é SSH
  "state": pane-state,
  "exit":  int / null,             ; código de saída quando state = exited
  "cols":  uint,
  "rows":  uint,
}

pane-state = &(
  running:      0,
  exited:       1,
  not-started:  2,   ; aba restaurada e ainda não iniciada, ou SSH dormente
  connecting:   3,
  reconnecting: 4,
  disconnected: 5,
  awaiting-desktop: 6,   ; parado numa pergunta respondida só no desktop (senha, passphrase, chave do servidor)
)
```

O estado da aba é derivado dos painéis pelo cliente, pela mesma regra do desktop (a aba mostra o estado do painel focado). Mudar a aba ativa ou o foco **no desktop** muda `active`/`focused` e gera um `workspace`. **O cliente nunca muda o foco do desktop**: nem a aba ativa de uma janela (`active`), nem o painel focado dentro de uma aba (`focused`), nem a janela do sistema que tem o foco (§7, "Aba ativa e janela do sistema"). `active` só muda por gesto no desktop ou quando uma ação remota remove a aba ativa de onde ela estava, e aí pela regra do desktop (§7). A aba que o cliente **exibe** é informada por `view-tab` (§4.4) e não muda o workspace.

### 4.2 `profiles`

Mandada depois do `welcome` e a cada recarga da configuração do host que mude os perfis.

```cddl
profiles = [11, {
  "list":    [* profile],
  "default": profile-id,
}]

profile = {
  "id":   profile-id,
  "name": tstr,
  "kind": &(local: 0, ssh: 1, wsl: 2, other: 3),
}
```

### 4.3 `theme`

Mandada depois do `welcome` e a cada troca de tema no host. O cliente pinta a grade com estas cores, para ela ficar igual à do desktop.

```cddl
theme = [12, {
  "fg":           rgb,
  "bg":           rgb,
  "cursor":       rgb,
  "selection_bg": rgb,
  "palette":      [16*16 rgb],     ; cores 0..15
  "group_colors": [+ rgb],         ; a paleta de cores de grupo, na ordem do host
}]
```

As cores 16 a 255 seguem a tabela padrão do xterm (cubo 6×6×6 e rampa de cinza), calculada pelo cliente.

### 4.4 `view-tab`

O cliente diz ao host qual aba ele **exibe** agora ([ADR-0011](../adr/0011-cliente-nao-muda-a-aba-ativa-e-view-tab.md)). É informação para o host mostrar a quem está no desktop (o Porecatu marca a aba na barra); **não é pedido**.

```cddl
view-tab = [13, { "tab": tab-id / null }]   ; null: o cliente não exibe nenhuma aba
```

- **Um estado por sessão.** Cada `view-tab` substitui o anterior. O estado começa `null` e morre com a sessão (`goodbye`, canal fechado, queda); o host não o guarda entre sessões.
- **Sem efeito no workspace.** O host **NÃO DEVE** agir sobre o workspace por causa de `view-tab`: não ativa a aba, não assina painel, não muda `active` nem `focused` e não manda `workspace`. Não há resposta de sucesso.
- **Aba desconhecida:** o host responde `error { code: unknown-tab }` e o estado vira `null`.
- **A aba fecha:** o host limpa o estado sem mandar nada; o cliente sabe pelo `workspace`.
- **A aba muda de janela ou de grupo:** o estado continua, porque o `tab-id` é estável (§3).
- **O cliente DEVE** mandar `view-tab { tab }` ao começar a exibir uma aba, e de novo em toda sessão nova (reconexão), porque o estado é da sessão. **DEVE** mandar `view-tab { tab: null }` ao parar de exibi-la, e **PODE** mandá-lo ao ir para segundo plano. Trocar de painel dentro da mesma aba não pede `view-tab`; mandar a mesma aba de novo não tem efeito.
- O host **não** repassa o estado a outros clientes no v0.

### 4.5 Sessões salvas

O Porecatu grava **sessões nomeadas**: a disposição de uma janela, salva com um nome pelo usuário no desktop ([ADR-0012](../adr/0012-sessoes-nomeadas-pelo-cliente.md)). O cliente pede a lista delas e restaura uma pela ação `restore-session` (§7). Salvar e excluir sessões são gestos só do desktop.

```cddl
list-sessions = [14, { "id": request-id }]

sessions = [15, {
  "id":   request-id,                ; o do list-sessions
  "mode": &(new-window: 0, append: 1),
  "list": [* named-session],         ; a salva mais recentemente primeiro
}]

named-session = {
  "session":  session-id,
  "name":     tstr,                  ; o nome dado no desktop, como está
  "readable": bool,                  ; false: o host não consegue restaurá-la
}
```

- **Só sob pedido.** O host lê as sessões salvas quando recebe `list-sessions` e responde com `sessions`, de mesmo `id`. Ele **não** manda `sessions` por conta própria, nem quando uma sessão é salva ou excluída no desktop: o cliente pede de novo quando precisa.
- **`mode`** é o valor atual da preferência do desktop sobre o destino da restauração: `new-window`, abrir uma janela nova; `append`, acrescentar a uma janela que já existe. O cliente o usa para saber se precisa perguntar a janela antes de mandar `restore-session`. Quem decide o destino é a preferência no momento da ação, não este valor (§7).
- **Ordem.** A da lista do desktop: a sessão salva mais recentemente primeiro.
- **Sessão ilegível.** Uma sessão que o host não consegue restaurar (arquivo corrompido, formato mais novo que o do host) vem com `readable: false` e, em `name`, o rótulo que o desktop mostra para ela. O cliente a lista sem oferecer a restauração.
- **Lista vazia** é `list: []`, nunca erro.
- **`session-id`** segue o §3: o mesmo arquivo recebe o mesmo id durante a vida do processo do host, e um id nunca é reaproveitado. Como os outros ids, só vale entre sessões com o mesmo `host_instance`.

## 5. Grade

### 5.1 Assinatura

O cliente recebe a grade de um painel **só enquanto o assina**. Um cliente tem no máximo `max_subscriptions` assinaturas (do `welcome`); o host lê o motor de um painel só enquanto algum cliente o assina.

```cddl
subscribe   = [20, { "pane": pane-id }]
unsubscribe = [21, { "pane": pane-id }]

subscription-ended = [27, {
  "pane":   pane-id,
  "reason": &(pane-closed: 0, host-limit: 1),
}]
```

Resposta a `subscribe`: um `grid-snapshot`, ou `error` (`unknown-pane`, `too-many-subscriptions`). Assinar de novo um painel já assinado manda um `grid-snapshot` novo (com o `seq` seguinte da mesma assinatura, §5.3), e é o jeito de o cliente pedir o estado completo de novo.

O painel que fecha encerra a assinatura com `subscription-ended`. Painel `not-started` pode ser assinado: o snapshot vem vazio, e o cliente oferece iniciar (`start-pane`, §7).

### 5.2 Linhas e estilos

```cddl
line  = [wrapped: bool, spans: [* span]]       ; wrapped: a linha continua na seguinte (quebra suave)
span  = [col: uint, width: 1 / 2, style: uint, text: tstr / [tstr]]
                                                ; tstr: span comum, um code point por célula
                                                ; [tstr]: um único grafema de vários code points, uma célula
style = [fg: color, bg: color, flags: uint]
color = null                                    ; cor padrão (fg ou bg do tema)
      / [0, uint .le 255]                       ; cor indexada
      / [1, rgb]                                ; cor direta

; flags de estilo
; 0x01 negrito   0x02 itálico   0x04 sublinhado   0x08 inverso
; 0x10 esmaecido 0x20 riscado   0x40 oculto
```

- Uma linha é uma lista de **spans** que não se sobrepõem, em ordem de coluna. Célula sem span é **espaço com estilo padrão** — é por isso que linha vazia é `[false, []]`.
- `width` é a largura em células de **cada** grafema do span: `1` para comuns, `2` para os de largura dupla (CJK, emoji). Um span nunca mistura larguras.
- **Span comum** (`text` é texto): um grafema por code point. O *i*-ésimo code point está na coluna `col + i × width`, e o span ocupa `(número de code points) × width` células. O host **NÃO DEVE** pôr num span comum um grafema de mais de um code point.
- **Span de grafema** (`text` é um array de exatamente um texto): o texto é **um único grafema** de mais de um code point (emoji com ZWJ, base com combinantes, bandeira, sequência com seletor de variação) e o span ocupa **uma** célula de `width` colunas, na coluna `col`. Array vazio, com mais de um elemento ou com texto vazio é fora do domínio.
- Assim o cliente acha a coluna de cada caractere **sem segmentar grafemas** e sem heurística: a forma do `text` diz se é um code point por célula ou um grafema numa célula ([ADR-0009](../adr/0009-grafema-no-fio-e-linhas-descartadas.md)).
- `style` é um índice na tabela `styles` da própria mensagem. As cores vão **não resolvidas** (padrão, indexada ou direta); quem resolve é o cliente, com o `theme`.

### 5.3 `grid-snapshot` e `grid-diff`

```cddl
grid-snapshot = [22, {
  "pane":    pane-id,
  "seq":     uint,                 ; monotônico por assinatura: = seq da mensagem de grade anterior + 1 (a primeira é 0)
  "cols":    uint,
  "rows":    uint,
  "history": uint,                 ; linhas de scrollback disponíveis acima da tela
  "dropped": uint,                 ; linhas já descartadas do topo do histórico deste painel (§5.5)
  "styles":  [* style],
  "lines":   [* line],             ; exatamente `rows` linhas, de cima para baixo
  "cursor":  cursor,
  "alt":     bool,                 ; tela alternativa ativa (TUI); sem scrollback
}]

grid-diff = [23, {
  "pane":    pane-id,
  "seq":     uint,                 ; = seq da mensagem anterior desta assinatura + 1
  "history": uint,
  "dropped": uint,
  "styles":  [* style],
  "lines":   [* [row: uint, line: line]],   ; só as linhas que mudaram
  "cursor":  cursor,
  "alt":     bool,
}]

cursor = {
  "row":   uint,
  "col":   uint,
  "shape": &(block: 0, underline: 1, beam: 2),
  "visible": bool,
}
```

- **`seq` é monotônico por assinatura e nunca reinicia**: snapshots e diffs compartilham a mesma sequência, começando em 0 na primeira mensagem de grade da assinatura. Uma assinatura dura do `subscribe` até o `unsubscribe` ou o `subscription-ended`; assinar de novo um painel já assinado **não** abre outra.
- O `grid-diff` se aplica sobre o estado que resultou da mensagem de grade anterior **da mesma assinatura** (`seq − 1`). Como os registros chegam em ordem, um `seq` fora de sequência só acontece por defeito de implementação: é **erro de protocolo** (`error { protocol }` e a sessão fecha). O cliente não tenta se recuperar dele por conta própria.
- Mudou o tamanho do painel, ou entrou/saiu da tela alternativa: o host manda `grid-snapshot`, nunca diff. O cliente **DEVE** aceitar snapshot a qualquer momento.
- O tamanho é **sempre o do desktop**. O cliente não tem como redimensionar o PTY; adapta zoom e rolagem horizontal.

### 5.4 Controle de fluxo

O host não empilha quadros numa rede lenta: manda o **estado mais recente**, não a história.

```cddl
grid-ack = [24, { "pane": pane-id, "seq": uint }]
```

- O cliente confirma cada `grid-snapshot` e `grid-diff` processado com `grid-ack`.
- O host mantém no máximo **2 mensagens de grade sem confirmação** por assinatura. Com a janela cheia, ele **acumula as linhas sujas** e, quando chega um `grid-ack`, manda **um** diff com tudo que mudou desde a última mensagem enviada.
- Teto de **30 mensagens de grade por segundo** por assinatura, mesmo com a janela livre.
- Painel ocioso não gera mensagem.

### 5.5 Scrollback

O cliente pede páginas do histórico; o host não as empurra.

```cddl
scrollback-request = [25, {
  "id":     request-id,
  "pane":   pane-id,
  "offset": uint,                  ; 0 = a linha logo acima do topo da tela
  "count":  uint .le 500,
}]

scrollback-page = [26, {
  "id":      request-id,
  "pane":    pane-id,
  "history": uint,                 ; linhas disponíveis no momento da resposta
  "dropped": uint,                 ; o mesmo contador do grid-snapshot/grid-diff, no momento da resposta
  "offset":  uint,
  "styles":  [* style],
  "lines":   [* line],             ; de cima para baixo: a última é a de `offset`
}]
```

O histórico anda enquanto a saída corre, e tem teto no host: quando ele enche, cada linha nova empurra a mais antiga para fora, e `history` fica constante. Por isso o host manda, junto de `history`, o contador **`dropped`**: quantas linhas já saíram do topo do histórico deste painel desde que o painel existe no host (monotônico, nunca diminui; zera só quando o histórico do painel é limpo, ex.: `clear`/reset do terminal, e então o host manda `grid-snapshot`).

Com os dois números, cada linha de histórico tem um **índice absoluto estável**, que não muda com a saída correndo:

```
índice absoluto = dropped + (history − 1 − offset)        ; 0 = a primeira linha que o painel já teve
```

O cliente guarda o scrollback pelo índice absoluto e converte de volta para `offset` ao pedir. Uma linha com índice absoluto menor que o `dropped` atual já não existe no host. Rolar no celular **não** move a rolagem do desktop. Na tela alternativa (`alt = true`), `history` é 0 e `dropped` repete o valor da tela normal.

## 6. Input

O cliente manda input **semântico**; quem o traduz em bytes é o host, com o mesmo codificador do teclado local — que conhece o modo de cursor da aplicação, o bracketed paste e a codificação de modificadores. Input só vale para painel **assinado** e em `running`; fora disso, `error { code: not-subscribed | pane-not-running }`.

```cddl
key = [30, {
  "pane": pane-id,
  "key":  named-key / tstr,        ; tstr: exatamente um valor escalar Unicode (ex.: "c" com Ctrl)
  "mods": uint,                    ; 0x01 Ctrl, 0x02 Alt, 0x04 Shift, 0x08 Meta
}]

named-key = &(
  enter: 0, tab: 1, backspace: 2, escape: 3,
  up: 4, down: 5, left: 6, right: 7,
  home: 8, end: 9, page-up: 10, page-down: 11, insert: 12, delete: 13,
  f1: 21, f2: 22, f3: 23, f4: 24, f5: 25, f6: 26,
  f7: 27, f8: 28, f9: 29, f10: 30, f11: 31, f12: 32,
)

text-input = [31, { "pane": pane-id, "text": tstr }]   ; texto digitado (inclusive composição de IME)
paste = [32, { "pane": pane-id, "text": tstr }]   ; colar: o host aplica as regras do colar local
```

No `paste`, o host aplica a mesma codificação do colar local (bracketed paste conforme o modo do programa) e **não pede confirmação ao cliente**: confirmar colagem de várias linhas, se houver, é decisão e responsabilidade do cliente, antes de mandar.

Atalho do Porecatu **não** é disparado por input remoto: `key` vai sempre ao terminal, nunca à tabela de ações do desktop. Ação é pedida por `action` (§7).

## 7. Ações

O cliente pede, o host executa **pelo mesmo caminho do gesto local** e responde. O host **não muda a aba ativa** do desktop por causa de uma ação remota: onde o gesto local ativaria uma aba, a ação remota não ativa (veja "Aba ativa e janela do sistema", abaixo).

```cddl
action = [40, { "id": request-id, "do": action-body }]

action-body =
    [0,  { "window": window-id, ? "group": group-id, ? "profile": profile-id }]  ; new-tab
  / [1,  { "tab": tab-id }]                                    ; close-tab
  / [2,  { "pane": pane-id }]                                  ; close-pane
  / [3,  { "tab": tab-id, "title": tstr / null }]              ; rename-tab (null limpa o título próprio)
  / [4,  { "tab": tab-id, "group": group-id / null }]          ; move-tab-to-group (null: sai do grupo)
  / [5,  { "tab": tab-id, "window": window-id }]               ; move-tab-to-window
  / [6,  { "tabs": [+ tab-id], "name": tstr, ? "color": uint }]; new-group
  / [7,  { "group": group-id, "name": tstr }]                  ; rename-group
  / [8,  { "group": group-id, "color": uint }]                 ; set-group-color
  / [9,  { "group": group-id, "collapsed": bool }]             ; collapse-group
  / [10, { "group": group-id, "window": window-id }]           ; move-group-to-window
  / [11, { "group": group-id }]                                ; ungroup
  / [12, { "pane": pane-id }]                                  ; start-pane
  ; 13 reservado: era activate-tab (0.4), removida na 0.5 (ADR-0011); nunca reaproveitado
  / [14, { "session": session-id, ? "window": window-id }]   ; restore-session

action-result = [41, {
  "id": request-id,
  ? "error":   error-code,         ; ausente = sucesso
  ? "created": tab-id / group-id,  ; new-tab devolve tab-id; new-group devolve group-id (os dois são uint: a ação de origem diz qual é)
}]
```

- **`new-tab`** sem `group` cria a aba onde o `tab.new` do desktop criaria, relativo à aba ativa daquela janela; com `group`, no fim do grupo. Sem `profile`, usa o padrão. Herda o `cwd` pela regra do desktop. A aba criada **não** fica ativa: a aba ativa da janela continua a mesma. Se o processo da aba não sobe, `invalid-argument`.
- **`close-tab` e `close-pane`.** `close-pane` do último painel de uma aba fecha a aba, como no desktop; fechar assim (ou por `close-tab`) a última aba da última janela é recusado com `last-tab`. **Aba fixada** é fechada como no desktop: o fechamento **sempre** pede confirmação (`confirm-request`) e, aceito, a aba é fechada **e desafixada**; vale também para o último painel (`close-pane`), que fecha a aba, não a reinicia. Uma fixada sem nada vivo recebe `confirm-request` com `kind = running-process` e `processes` vazio.
- **`rename-tab`** em aba fixada é aceito.
- **`move-tab-to-group`** com um grupo de **outra janela** responde `unknown-group` (para a janela da aba, o grupo não existe, como no `new-tab` com um grupo de outra janela). Aba fixada, com grupo ou com `null`, responde `invalid-argument`. `group: null` com a aba já fora de grupo é sucesso, sem efeito.
- **`new-group`** exige que todas as abas estejam na **mesma janela** e nenhuma seja fixada; senão, `invalid-argument`. O grupo nasce na posição da **primeira das abas na ordem da trilha** (não na da primeira da lista), e a ordem das abas dentro do grupo segue a da trilha, não a da lista.
- **`collapse-group`**: expandir (`collapsed: false`) **não** ativa nada; a aba ativa da janela continua a mesma. Colapsar o grupo que contém a aba ativa move a aba ativa pela regra do desktop, porque ela deixa de estar à vista.
- **`move-*-to-window`** move com o processo vivo, como o arraste e o "Mover para janela…" do desktop, e a aba (ou o grupo) chega à janela de destino **sem ser ativada**; se a aba ativa da origem saiu, a origem ativa a vizinha pela regra do desktop. Para a **mesma janela** de origem, `invalid-argument`. Mover a última aba de uma janela é permitido (a janela que esvazia fecha). Aba fixada vai fixada. Não há "destacar numa janela nova" no v0.
- **`start-pane`** inicia painel `not-started` ou conecta SSH dormente, como o clique no desktop, **sem ativar** a aba do painel. Painel que não está `not-started` e não é SSH dormente: `invalid-argument`. Perguntas do backend (senha, passphrase, chave do servidor desconhecida) aparecem **só no desktop** no v0; enquanto a pergunta espera resposta, o cliente vê o painel em `awaiting-desktop` (§4.1).
- **`restore-session`** restaura uma sessão salva (§4.5) pelo mesmo caminho da restauração do desktop ([ADR-0012](../adr/0012-sessoes-nomeadas-pelo-cliente.md)). O destino é a preferência do desktop **no momento da ação**, a mesma que `sessions.mode` informa:
  - **`new-window`**: o host abre uma janela nova com a disposição da sessão e ignora `window`. A janela tem o tamanho salvo e a posição em cascata a partir da janela de foco mais recente. A janela nova **nasce sem foco**: o host **NÃO DEVE** levantá-la, ativá-la nem pô-la à frente das outras na ordem de foco (`workspace.windows`). Dentro dela, a aba ativa é a que era ativa na sessão.
  - **`append`**: o host acrescenta os grupos e as abas da sessão **ao fim da trilha** da janela `window`, depois de tudo o que ela já tinha; grupo com o nome de um grupo existente não se funde com ele. Sem `window`, `invalid-argument`.
  - **Sem ativar e sem fechar.** Nos dois destinos, a aba ativa de toda janela que já existia continua a mesma. O host **NÃO DEVE** ativar a aba que era ativa na sessão nem fechar aba alguma, inclusive a aba que o gesto local fecharia ao restaurar.
  - Tema, zoom e abas fixadas gravados na sessão **não** são aplicados. Cada aba se comporta como uma aba restaurada no desktop (diretório com fallback, árvore de painéis, início adiado conforme a configuração do host).
  - O sucesso **não** traz `created`; a janela ou as abas chegam pelo `workspace` seguinte.
  - Erros: a sessão que não existe mais (inclusive a que foi excluída no desktop depois da lista) é `unknown-session`; o arquivo que o host não consegue ler é `session-unreadable`, verificado por último, depois de todas as outras checagens. `busy` vale para a janela `window` no `append` e para o desktop inteiro no `new-window` (ex.: arraste entre janelas em curso).
- **Fechar com confirmação.** Quando o gesto local pediria confirmação (processo em primeiro plano, conexão SSH aberta), o host pergunta **ao cliente que pediu**, não ao desktop:

  ```cddl
  confirm-request = [42, {
    "id":        request-id,       ; o da ação
    "kind":      &(running-process: 0, ssh-connection: 1),
    "processes": [* tstr],         ; nomes dos processos, para o cliente listar
  }]

  confirm = [43, { "id": request-id, "accept": bool }]
  ```

  Sem resposta em 60 s, a ação termina com `action-result { error: confirm-timeout }`. Recusa termina com `action-result { error: cancelled }`. A confirmação vai **sempre ao cliente**; o host nunca abre diálogo no desktop por causa de uma ação remota.

**Ordem das checagens.** Para toda ação: primeiro os ids desconhecidos (`unknown-*`, na ordem dos campos da ação), depois `busy`, depois as regras da própria ação (`invalid-argument`, `last-tab`, e por fim `session-unreadable`, que exige ler o arquivo) e por último a confirmação.

### Aba ativa e janela do sistema

A 0.3 não deixava o cliente mudar a aba ativa; a 0.4 deixou ([ADR-0010](../adr/0010-aba-ativa-pelo-cliente.md)); a 0.5 volta a não deixar ([ADR-0011](../adr/0011-cliente-nao-muda-a-aba-ativa-e-view-tab.md)):

1. **A aba ativa não muda por pedido do cliente.** O host **NÃO DEVE** ativar aba por causa de uma ação remota (`new-tab`, `collapse-group`, `move-*-to-window`, `start-pane`, `restore-session`) nem de `view-tab`. A aba ativa só muda quando a ação **remove** a aba ativa de onde ela estava (fechar a aba ou o último painel dela, movê-la para outra janela, colapsar o grupo que a contém): o desktop ativa a vizinha pela regra dele, e o host manda o `workspace` com o `active` novo.
2. **Janela do sistema operacional, nunca.** O host **NÃO DEVE** levantar, desminimizar nem dar foco a uma janela do sistema por causa do cliente, nem na janela de destino de `move-*-to-window`, nem na janela que `restore-session` abre.
3. **O painel focado dentro da aba** (`focused`) não é mudado pelo cliente: não há ação para isso.

**Resultado de ação sempre vem em `action-result`**, com o mesmo `id`, nunca numa mensagem `error` avulsa — inclusive `busy`, `last-tab`, `unknown-*` e `invalid-argument`.

Fora do v0, de propósito: fixar e desafixar, salvar e excluir sessões nomeadas, configurações, gerenciador de tarefas, abrir janela vazia, mudar a aba ativa, mudar o painel focado dentro de uma aba e dar foco a uma janela do sistema.

## 8. Pareamento

Só existem numa sessão aberta por um handshake `XX` ([02](02-handshake-e-pareamento.md) §3), e são a primeira mensagem de aplicação do host nela. Depois de `pair-rejected`, o host fecha o canal com `close-channel { normal }`.

```cddl
pair-accepted = [60, { "host_name": tstr }]
pair-rejected = [61, { "reason": &(token-invalid: 0, user-declined: 1, timeout: 2) }]
```

## 9. Eventos e erros

```cddl
bell  = [50, { "pane": pane-id }]           ; campainha num painel (transitório; o indicador fica no workspace)

error = [70, {
  "code": error-code,
  ? "id": request-id,                       ; quando é resposta a um pedido com id (scrollback-request)
  ? "pane": pane-id,                        ; quando é resposta a subscribe, unsubscribe, grid-ack, key, text-input ou paste
}]

error-code = &(
  protocol:               0,
  not-paired:             1,
  ; 2 reservado: a revogação de uma sessão viva é `goodbye { revoked }` (§2)
  unknown-window:         3,
  unknown-group:          4,
  unknown-tab:            5,
  unknown-pane:           6,
  unknown-profile:        7,
  pane-not-running:       8,
  not-subscribed:         9,
  too-many-subscriptions: 10,
  invalid-argument:       11,
  busy:                   12,   ; o desktop está num estado que impede a ação (ex.: diálogo modal aberto)
  confirm-timeout:        13,
  cancelled:              14,
  unsupported:            15,
  last-tab:               16,   ; a ação fecharia a última aba da última janela, encerrando o host
  unknown-session:        17,   ; a sessão salva não existe (mais) no host
  session-unreadable:     18,   ; o host não consegue ler a sessão salva para restaurá-la
)
```

## 10. Tabela de mensagens

| Tipo | Nome | Sentido | Seção |
|---|---|---|---|
| 0 | `hello` | C → H | §2 |
| 1 | `welcome` | H → C | §2 |
| 2 | `incompatible` | H → C | §2 |
| 3 | `goodbye` | ambos | §2 |
| 10 | `workspace` | H → C | §4.1 |
| 11 | `profiles` | H → C | §4.2 |
| 12 | `theme` | H → C | §4.3 |
| 13 | `view-tab` | C → H | §4.4 |
| 14 | `list-sessions` | C → H | §4.5 |
| 15 | `sessions` | H → C | §4.5 |
| 20 | `subscribe` | C → H | §5.1 |
| 21 | `unsubscribe` | C → H | §5.1 |
| 22 | `grid-snapshot` | H → C | §5.3 |
| 23 | `grid-diff` | H → C | §5.3 |
| 24 | `grid-ack` | C → H | §5.4 |
| 25 | `scrollback-request` | C → H | §5.5 |
| 26 | `scrollback-page` | H → C | §5.5 |
| 27 | `subscription-ended` | H → C | §5.1 |
| 30 | `key` | C → H | §6 |
| 31 | `text-input` | C → H | §6 |
| 32 | `paste` | C → H | §6 |
| 40 | `action` | C → H | §7 |
| 41 | `action-result` | H → C | §7 |
| 42 | `confirm-request` | H → C | §7 |
| 43 | `confirm` | C → H | §7 |
| 50 | `bell` | H → C | §9 |
| 60 | `pair-accepted` | H → C | §8 |
| 61 | `pair-rejected` | H → C | §8 |
| 70 | `error` | ambos | §9 |

As faixas livres entre os grupos são para mensagens de versões menores futuras, no grupo a que pertencem.

## Anexo A — Schema CDDL completo

A **fonte** do schema são os blocos `cddl` de [01](01-transporte-e-relay.md), [02](02-handshake-e-pareamento.md) e deste documento. O arquivo `schema/porecatu-remote.cddl` é **gerado** deles pelo script `scripts/extract-cddl.py`, que acrescenta `* tstr => any` a todo mapa de corpo (§1) e uma regra raiz por categoria de vetor; o verificador de documentação reprova se o arquivo estiver fora de sincronia, e o CI valida todos os vetores de teste ([vetores-de-teste](vetores-de-teste/README.md)) contra ele.

```cddl
message = hello / welcome / incompatible / goodbye
        / workspace / profiles / theme / view-tab / list-sessions / sessions
        / subscribe / unsubscribe / grid-snapshot / grid-diff / grid-ack
        / scrollback-request / scrollback-page / subscription-ended
        / key / text-input / paste
        / action / action-result / confirm-request / confirm
        / bell / pair-accepted / pair-rejected / error

rgb = uint .le 0xFFFFFF
```
