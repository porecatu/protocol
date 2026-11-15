# 03 — Mensagens

**Versão da especificação:** 0.1 (rascunho)

Este documento cobre a camada 4: as mensagens de aplicação que host e cliente trocam dentro da sessão Noise. A escolha de CBOR e CDDL está no [ADR-0003](../adr/0003-cbor-com-schema-em-cddl.md).

## 1. Convenções

- Cada mensagem é **um item CBOR** (RFC 8949), em codificação determinística (§4.2.1), remontado dos registros ([02](02-handshake-e-pareamento.md) §5).
- Toda mensagem é um array `[tipo, corpo]`, com `tipo` inteiro (tabela do §10) e `corpo` um mapa de chaves de texto.
- **Chave desconhecida num mapa é ignorada.** É o que permite acrescentar campo numa versão menor ([04](04-versionamento.md)). Chave obrigatória ausente é erro de protocolo.
- **Tipo de mensagem desconhecido é erro de protocolo.** Mensagem nova numa versão menor só é mandada a quem anunciou essa versão menor no `hello`/`welcome`.
- Erro de protocolo (CBOR inválido, campo obrigatório ausente, tipo desconhecido, valor fora do domínio): quem detecta manda `error { code: protocol }` e fecha a sessão.
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
| `request-id` | sessão | Escolhido pelo cliente, único entre os pedidos pendentes da sessão |

Os ids do protocolo **não são** os ids internos do Porecatu (que são por janela e mudam quando uma aba muda de janela). O host mantém o mapeamento; o cliente nunca vê id interno. Reiniciar o Porecatu reinicia todos os ids.

```cddl
window-id  = uint
group-id   = uint
tab-id     = uint
pane-id    = uint
profile-id = tstr
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
  "ratio": float32,                ; fração do primeiro filho, 0 < ratio < 1
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

O estado da aba é derivado dos painéis pelo cliente, pela mesma regra do desktop (a aba mostra o estado do painel focado). Mudar a aba ativa ou o foco **no desktop** muda `active`/`focused` e gera um `workspace`; **o cliente nunca muda o foco do desktop** (§7).

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

Resposta a `subscribe`: um `grid-snapshot`, ou `error` (`unknown-pane`, `too-many-subscriptions`). Assinar de novo um painel já assinado manda um `grid-snapshot` novo, e é o jeito de o cliente se ressincronizar.

O painel que fecha encerra a assinatura com `subscription-ended`. Painel `not-started` pode ser assinado: o snapshot vem vazio, e o cliente oferece iniciar (`start-pane`, §7).

### 5.2 Linhas e estilos

```cddl
line  = [wrapped: bool, spans: [* span]]       ; wrapped: a linha continua na seguinte (quebra suave)
span  = [col: uint, width: 1 / 2, style: uint, text: tstr]
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
- **Um grafema por code point**, salvo exceção: o host **DEVE** pôr todo grafema de mais de um code point (emoji com ZWJ, base com combinantes) **sozinho num span**, com `text` igual ao grafema inteiro. Assim o cliente acha a coluna de cada caractere sem segmentar grafemas: o *i*-ésimo code point de um span comum está em `col + i × width`.
- `style` é um índice na tabela `styles` da própria mensagem. As cores vão **não resolvidas** (padrão, indexada ou direta); quem resolve é o cliente, com o `theme`.

### 5.3 `grid-snapshot` e `grid-diff`

```cddl
grid-snapshot = [22, {
  "pane":    pane-id,
  "seq":     uint,                 ; reinicia a cada snapshot
  "cols":    uint,
  "rows":    uint,
  "history": uint,                 ; linhas de scrollback disponíveis acima da tela
  "styles":  [* style],
  "lines":   [* line],             ; exatamente `rows` linhas, de cima para baixo
  "cursor":  cursor,
  "alt":     bool,                 ; tela alternativa ativa (TUI); sem scrollback
}]

grid-diff = [23, {
  "pane":    pane-id,
  "seq":     uint,                 ; = seq da mensagem anterior desta assinatura + 1
  "history": uint,
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

- O `grid-diff` se aplica sobre o estado que resultou da mensagem anterior **da mesma assinatura** (`seq − 1`). `seq` fora de sequência é erro de protocolo.
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
  "offset":  uint,
  "styles":  [* style],
  "lines":   [* line],             ; de cima para baixo: a última é a de `offset`
}]
```

O histórico anda enquanto a saída corre: um `history` maior que o da página anterior significa que linhas novas empurraram as antigas para cima, e o cliente corrige o deslocamento pela diferença. Rolar no celular **não** move a rolagem do desktop. Na tela alternativa (`alt = true`), `history` é 0.

## 6. Input

O cliente manda input **semântico**; quem o traduz em bytes é o host, com o mesmo codificador do teclado local — que conhece o modo de cursor da aplicação, o bracketed paste e a codificação de modificadores. Input só vale para painel **assinado** e em `running`; fora disso, `error { code: not-subscribed | pane-not-running }`.

```cddl
key = [30, {
  "pane": pane-id,
  "key":  named-key / tstr,        ; tstr: um único caractere (ex.: "c" com Ctrl)
  "mods": uint,                    ; 0x01 Ctrl, 0x02 Alt, 0x04 Shift, 0x08 Meta
}]

named-key = &(
  enter: 0, tab: 1, backspace: 2, escape: 3,
  up: 4, down: 5, left: 6, right: 7,
  home: 8, end: 9, page-up: 10, page-down: 11, insert: 12, delete: 13,
  f1: 21, f2: 22, f3: 23, f4: 24, f5: 25, f6: 26,
  f7: 27, f8: 28, f9: 29, f10: 30, f11: 31, f12: 32,
)

text  = [31, { "pane": pane-id, "text": tstr }]   ; texto digitado (inclusive composição de IME)
paste = [32, { "pane": pane-id, "text": tstr }]   ; colar: o host aplica as regras do colar local
```

No `paste`, o host aplica a mesma codificação do colar local (bracketed paste conforme o modo do programa) e **não pede confirmação ao cliente**: confirmar colagem de várias linhas, se houver, é decisão e responsabilidade do cliente, antes de mandar.

Atalho do Porecatu **não** é disparado por input remoto: `key` vai sempre ao terminal, nunca à tabela de ações do desktop. Ação é pedida por `action` (§7).

## 7. Ações

O cliente pede, o host executa **pelo mesmo caminho do gesto local** e responde. O host **não muda o foco nem a aba ativa** do desktop por causa de uma ação remota: aba criada pelo celular nasce sem roubar o foco de quem está no desktop.

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

action-result = [41, {
  "id": request-id,
  ? "error":   error-code,         ; ausente = sucesso
  ? "created": tab-id / group-id,  ; new-tab devolve tab-id; new-group devolve group-id (os dois são uint: a ação de origem diz qual é)
}]
```

- **`new-tab`** sem `group` cria a aba onde o `tab.new` do desktop criaria, relativo à aba ativa daquela janela; com `group`, no fim do grupo. Sem `profile`, usa o padrão. Herda o `cwd` pela regra do desktop.
- **`close-pane`** do último painel de uma aba fecha a aba, como no desktop (salvo aba fixada, que fica com o painel reiniciado pela regra do desktop); fechar assim a última aba da última janela é recusado com `last-tab`, como `close-tab`.
- **`new-group`** exige que todas as abas estejam na **mesma janela** e nenhuma seja fixada; senão, `invalid-argument`. O grupo nasce na posição da primeira aba da lista.
- **`move-*-to-window`** move com o processo vivo, como o arraste e o "Mover para janela…" do desktop. Não há "destacar numa janela nova" no v0.
- **`start-pane`** inicia painel `not-started` ou conecta SSH dormente, como o clique no desktop. Perguntas do backend (senha, passphrase, chave do servidor desconhecida) aparecem **só no desktop** no v0; o cliente vê o painel em `connecting` até resolver.
- **Fechar com confirmação.** Quando o gesto local pediria confirmação (processo em primeiro plano, conexão SSH aberta), o host pergunta **ao cliente que pediu**, não ao desktop:

  ```cddl
  confirm-request = [42, {
    "id":        request-id,       ; o da ação
    "kind":      &(running-process: 0, ssh-connection: 1),
    "processes": [* tstr],         ; nomes dos processos, para o cliente listar
  }]

  confirm = [43, { "id": request-id, "accept": bool }]
  ```

  Sem resposta em 60 s, a ação termina com `error { confirm-timeout }`. Recusa termina com `cancelled`.

Fora do v0, de propósito: fixar e desafixar, sessões nomeadas, configurações, gerenciador de tarefas, abrir janela nova, mudar o foco do desktop.

## 8. Pareamento

Só existem dentro de um handshake `XX` ([02](02-handshake-e-pareamento.md) §3).

```cddl
pair-accepted = [60, { "host_name": tstr }]
pair-rejected = [61, { "reason": &(token-invalid: 0, user-declined: 1, timeout: 2) }]
```

## 9. Eventos e erros

```cddl
bell  = [50, { "pane": pane-id }]           ; campainha num painel (transitório; o indicador fica no workspace)

error = [70, {
  "code": error-code,
  ? "id": request-id,                       ; quando é resposta a um pedido
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
| 20 | `subscribe` | C → H | §5.1 |
| 21 | `unsubscribe` | C → H | §5.1 |
| 22 | `grid-snapshot` | H → C | §5.3 |
| 23 | `grid-diff` | H → C | §5.3 |
| 24 | `grid-ack` | C → H | §5.4 |
| 25 | `scrollback-request` | C → H | §5.5 |
| 26 | `scrollback-page` | H → C | §5.5 |
| 27 | `subscription-ended` | H → C | §5.1 |
| 30 | `key` | C → H | §6 |
| 31 | `text` | C → H | §6 |
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

O schema normativo é o conjunto dos blocos `cddl` deste documento e de [02](02-handshake-e-pareamento.md). Quando as implementações de referência existirem, o arquivo `schema/porecatu-remote.cddl` passa a ser a fonte única, gerado a partir destes blocos e conferido pelo CI contra os vetores de teste ([vetores-de-teste](vetores-de-teste/README.md)).

```cddl
message = hello / welcome / incompatible / goodbye
        / workspace / profiles / theme
        / subscribe / unsubscribe / grid-snapshot / grid-diff / grid-ack
        / scrollback-request / scrollback-page / subscription-ended
        / key / text / paste
        / action / action-result / confirm-request / confirm
        / bell / pair-accepted / pair-rejected / error

rgb = uint .le 0xFFFFFF
```
