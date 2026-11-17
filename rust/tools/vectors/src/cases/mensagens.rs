// SPDX-License-Identifier: GPL-3.0-or-later
//! Message vectors (`mensagens/<type>/`): the 26 messages of specification 03
//! plus the `pair-request` of 02 section 3.2.
//!
//! For each message the cases written by hand here are a **minimal** body
//! (only the required keys), a **complete** one (every optional key, or
//! extreme values when the message has none) and a few extras. The other
//! three categories of the README table are **derived** from the minimal body:
//!
//! - `chave-desconhecida`: a key `"zz"` added at its canonical place; valid,
//!   with `reencode_hex` equal to the minimal bytes;
//! - `sem-<chave>`: each required key removed in turn; refused;
//! - `nao-canonico`: two keys of the body swapped; refused.
//!
//! Values reuse the canonical inputs of the codec tests of the Rust and
//! TypeScript implementations (pane 7, seq 3, the `界` cell, and so on), so
//! the hex of these vectors is the hex both sides already checked by hand.

use porecatu_protocol::cbor::{self, Value};
use serde_json::{Value as Json, json};

use super::{Generated, bytes, map, rejected, seq, to_value, valid};
use crate::typed_json::from_value;
use crate::vector::Layer;

/// Largest integer of the wire (`2^53 - 1`).
const MAX: u64 = (1 << 53) - 1;

/// A message of the table of 03 section 10 (or the `pair-request`).
struct Msg {
    /// Folder under `mensagens/`.
    folder: &'static str,
    /// Message type number; `None` for the `pair-request`, a bare map.
    code: Option<u64>,
    /// CDDL root rule.
    raiz: &'static str,
    /// Body with only the required keys.
    minimal: Json,
    /// Body with every optional key, or extreme values.
    complete: Json,
    /// Description of the complete case.
    complete_desc: &'static str,
    /// Extra cases.
    extras: Vec<Extra>,
}

/// An extra case: a body, valid or not.
struct Extra {
    file: String,
    desc: String,
    body: Json,
    valid: bool,
}

fn ok(file: &str, desc: &str, body: Json) -> Extra {
    Extra {
        file: file.to_owned(),
        desc: desc.to_owned(),
        body,
        valid: true,
    }
}

fn bad(file: &str, desc: &str, body: Json) -> Extra {
    Extra {
        file: file.to_owned(),
        desc: desc.to_owned(),
        body,
        valid: false,
    }
}

// ---------------------------------------------------------------------------
// Shared pieces
// ---------------------------------------------------------------------------

fn pane(id: u64) -> Json {
    pane_full(id, "a", Json::Null, 0, Json::Null, 80, 24)
}

fn pane_full(
    id: u64,
    title: &str,
    cwd: Json,
    state: u64,
    exit: Json,
    cols: u64,
    rows: u64,
) -> Json {
    map(vec![
        ("pane", json!(id)),
        ("title", json!(title)),
        ("cwd", cwd),
        ("state", json!(state)),
        ("exit", exit),
        ("cols", json!(cols)),
        ("rows", json!(rows)),
    ])
}

fn tab(id: u64, layout: Json, focused: u64) -> Json {
    map(vec![
        ("tab", json!(id)),
        ("title", json!("a")),
        ("custom", json!(false)),
        ("profile", Json::Null),
        ("remote", Json::Null),
        ("activity", json!(false)),
        ("bell", json!(false)),
        ("layout", layout),
        ("focused", json!(focused)),
    ])
}

fn window(id: u64, title: &str, pinned: Vec<Json>, items: Vec<Json>, active: Json) -> Json {
    map(vec![
        ("id", json!(id)),
        ("title", json!(title)),
        ("pinned", Json::Array(pinned)),
        ("items", Json::Array(items)),
        ("active", active),
    ])
}

fn workspace(rev: u64, windows: Vec<Json>) -> Json {
    map(vec![("rev", json!(rev)), ("windows", Json::Array(windows))])
}

/// A workspace of one window with one item.
fn workspace_of(item: Json) -> Json {
    workspace(1, vec![window(1, "a", vec![], vec![item], Json::Null)])
}

fn split(dir: u64, ratio: u64, first: Json, second: Json) -> Json {
    map(vec![
        ("dir", json!(dir)),
        ("ratio", json!(ratio)),
        ("first", first),
        ("second", second),
    ])
}

fn group(id: u64, name: &str, color: u64, collapsed: bool, tabs: Vec<Json>) -> Json {
    map(vec![
        ("group", json!(id)),
        ("name", json!(name)),
        ("color", json!(color)),
        ("collapsed", json!(collapsed)),
        ("tabs", Json::Array(tabs)),
    ])
}

fn version(major: u64, minor: u64) -> Json {
    json!([major, minor])
}

fn profile(id: &str, name: &str, kind: u64) -> Json {
    map(vec![
        ("id", json!(id)),
        ("name", json!(name)),
        ("kind", json!(kind)),
    ])
}

fn palette() -> Json {
    json!((0..16).collect::<Vec<u64>>())
}

fn theme(fg: u64, bg: u64, palette: Json, group_colors: Json) -> Json {
    map(vec![
        ("fg", json!(fg)),
        ("bg", json!(bg)),
        ("cursor", json!(0x5e_d3bc_u64)),
        ("selection_bg", json!(0x2b_3a4a_u64)),
        ("palette", palette),
        ("group_colors", group_colors),
    ])
}

// ---- grid -----------------------------------------------------------------

fn style(fg: Json, bg: Json, flags: u64) -> Json {
    json!([fg, bg, flags])
}

fn plain() -> Json {
    style(Json::Null, Json::Null, 0)
}

/// A plain span: `text` is a `tstr`, one code point per cell.
fn span(col: u64, width: u64, style: u64, text: &str) -> Json {
    json!([col, width, style, text])
}

/// A grapheme span: `text` is `[tstr]`, one grapheme in one cell.
fn gspan(col: u64, width: u64, style: u64, text: &str) -> Json {
    json!([col, width, style, [text]])
}

fn line(wrapped: bool, spans: Vec<Json>) -> Json {
    json!([wrapped, spans])
}

fn cursor() -> Json {
    map(vec![
        ("row", json!(0)),
        ("col", json!(2)),
        ("shape", json!(0)),
        ("visible", json!(true)),
    ])
}

#[derive(Clone, Copy)]
enum Grid {
    Snapshot,
    Diff,
    Page,
}

/// A grid message body over the given style table and lines.
fn grid(kind: Grid, cols: u64, styles: Vec<Json>, lines: Vec<Json>) -> Json {
    match kind {
        Grid::Snapshot => map(vec![
            ("pane", json!(7)),
            ("seq", json!(0)),
            ("cols", json!(cols)),
            ("rows", json!(lines.len())),
            ("history", json!(0)),
            ("dropped", json!(0)),
            ("styles", Json::Array(styles)),
            ("lines", Json::Array(lines)),
            ("cursor", cursor()),
            ("alt", json!(false)),
        ]),
        Grid::Diff => {
            let changed: Vec<Json> = lines
                .into_iter()
                .enumerate()
                .map(|(row, line)| json!([row, line]))
                .collect();
            map(vec![
                ("pane", json!(7)),
                ("seq", json!(1)),
                ("history", json!(0)),
                ("dropped", json!(2)),
                ("styles", Json::Array(styles)),
                ("lines", Json::Array(changed)),
                ("cursor", cursor()),
                ("alt", json!(false)),
            ])
        }
        Grid::Page => map(vec![
            ("id", json!(1)),
            ("pane", json!(7)),
            ("history", json!(100)),
            ("dropped", json!(3)),
            ("offset", json!(10)),
            ("styles", Json::Array(styles)),
            ("lines", Json::Array(lines)),
        ]),
    }
}

/// A grid message of one line whose only span has the given `text` value.
fn bad_text(kind: Grid, text: Json) -> Json {
    grid(
        kind,
        1,
        vec![plain()],
        vec![line(false, vec![json!([0, 1, 0, text])])],
    )
}

/// The extra cases the README asks for on grid messages, plus the spans
/// whose `text` is outside the domain.
fn grid_extras(kind: Grid) -> Vec<Extra> {
    let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
    vec![
        ok(
            "largura-dupla",
            "uma célula de largura dupla (界, width 2) ao lado de uma comum",
            grid(
                kind,
                4,
                vec![plain()],
                vec![line(false, vec![span(0, 2, 0, "界"), span(2, 1, 0, "a")])],
            ),
        ),
        ok(
            "grafema-varios-codepoints",
            "grafemas de vários code points, cada um num span de grafema `[tstr]` (família com ZWJ e e + acento combinante)",
            grid(
                kind,
                4,
                vec![plain()],
                vec![line(
                    false,
                    vec![gspan(0, 2, 0, family), gspan(2, 1, 0, "e\u{301}")],
                )],
            ),
        ),
        bad(
            "texto-array-vazio",
            "span com text `[]`: um span de grafema tem exatamente um texto",
            bad_text(kind, json!([])),
        ),
        bad(
            "texto-array-de-dois",
            "span com text `[\"a\", \"b\"]`: dois grafemas num span de grafema",
            bad_text(kind, json!(["a", "b"])),
        ),
        bad(
            "texto-array-de-texto-vazio",
            "span com text `[\"\"]`: o grafema não pode ser vazio",
            bad_text(kind, json!([""])),
        ),
        ok(
            "linha-vazia",
            "linhas sem nenhum span, uma contínua e outra com quebra suave",
            grid(
                kind,
                4,
                vec![plain()],
                vec![line(false, vec![]), line(true, vec![])],
            ),
        ),
        ok(
            "tres-formas-de-cor",
            "as três formas de color: null (padrão), [0, índice] e [1, rgb], com todas as flags",
            grid(
                kind,
                4,
                vec![
                    plain(),
                    style(json!([0, 196]), json!([1, 0x11_2233_u64]), 1),
                    style(json!([1, 0xff_ffff_u64]), json!([0, 0]), 0x7f),
                ],
                vec![line(
                    false,
                    vec![span(0, 1, 0, "a"), span(1, 1, 1, "b"), span(2, 1, 2, "c")],
                )],
            ),
        ),
    ]
}

// ---------------------------------------------------------------------------
// The table
// ---------------------------------------------------------------------------

// A long, flat catalogue of cases: one push per message reads better than a giant `vec![]`.
#[allow(clippy::vec_init_then_push)]
fn messages() -> Vec<Msg> {
    let mut all = Vec::new();

    all.push(Msg {
        folder: "hello",
        code: Some(0),
        raiz: "message",
        minimal: map(vec![
            ("version", version(0, 1)),
            ("app", json!("porecatu-remote")),
            ("app_version", json!("0.1.0")),
        ]),
        complete: map(vec![
            ("version", version(0, 1)),
            ("app", json!("Porecatu Remote (beta) é")),
            ("app_version", json!("1.2.3-rc.1+build.5")),
        ]),
        complete_desc: "hello com texto fora do ASCII e um semver com pré-lançamento",
        extras: vec![],
    });

    all.push(Msg {
        folder: "welcome",
        code: Some(1),
        raiz: "message",
        minimal: map(vec![
            ("version", version(0, 1)),
            ("host_name", json!("desk")),
            ("porecatu_version", json!("0.9.11")),
            ("max_subscriptions", json!(4)),
            ("host_instance", bytes(&[0xab; 16])),
        ]),
        complete: map(vec![
            ("version", version(0, 1)),
            ("host_name", json!("Mesa do Léo (casa)")),
            ("porecatu_version", json!("0.9.11-beta.1")),
            ("max_subscriptions", json!(MAX)),
            ("host_instance", bytes(&seq(0, 16))),
        ]),
        complete_desc: "welcome com nome fora do ASCII e max_subscriptions no maior inteiro",
        extras: vec![bad(
            "max-subscriptions-zero",
            "max_subscriptions 0 (o domínio é >= 1)",
            map(vec![
                ("version", version(0, 1)),
                ("host_name", json!("desk")),
                ("porecatu_version", json!("0.9.11")),
                ("max_subscriptions", json!(0)),
                ("host_instance", bytes(&[0xab; 16])),
            ]),
        )],
    });

    all.push(Msg {
        folder: "incompatible",
        code: Some(2),
        raiz: "message",
        minimal: map(vec![("version", version(0, 1))]),
        complete: map(vec![("version", version(MAX, MAX))]),
        complete_desc: "incompatible com a versão no maior inteiro nos dois campos",
        extras: vec![],
    });

    all.push(Msg {
        folder: "goodbye",
        code: Some(3),
        raiz: "message",
        minimal: map(vec![("reason", json!(2))]),
        complete: map(vec![("reason", json!(3))]),
        complete_desc: "goodbye host-quitting, o último valor do enum",
        extras: vec![
            ok("normal", "goodbye normal", map(vec![("reason", json!(0))])),
            ok(
                "remote-disabled",
                "goodbye remote-disabled",
                map(vec![("reason", json!(1))]),
            ),
            bad(
                "reason-fora-do-dominio",
                "goodbye com reason 9, fora do domínio 0..3",
                map(vec![("reason", json!(9))]),
            ),
        ],
    });

    // ---- workspace, profiles, theme --------------------------------------
    let full_layout = split(
        1,
        65535,
        pane(3),
        split(
            0,
            1,
            pane_full(4, "ssh", json!("/home/a"), 1, json!(-1), 120, 30),
            pane_full(5, "a", Json::Null, 6, Json::Null, 80, 24),
        ),
    );
    let full_tab = map(vec![
        ("tab", json!(2)),
        ("title", json!("a")),
        ("custom", json!(true)),
        ("profile", json!("auto:pwsh")),
        ("remote", json!("me@host")),
        ("activity", json!(true)),
        ("bell", json!(true)),
        ("layout", full_layout),
        ("focused", json!(4)),
    ]);
    all.push(Msg {
        folder: "workspace",
        code: Some(10),
        raiz: "message",
        minimal: workspace(
            1,
            vec![window(
                1,
                "a",
                vec![],
                vec![tab(2, pane(3), 3)],
                json!(2),
            )],
        ),
        complete: workspace(
            MAX,
            vec![
                window(
                    1,
                    "a",
                    vec![tab(10, pane(11), 11)],
                    vec![
                        full_tab,
                        group(
                            6,
                            "work",
                            2,
                            true,
                            vec![tab(7, pane(8), 8), tab(9, pane(12), 12)],
                        ),
                    ],
                    Json::Null,
                ),
                window(2, "", vec![], vec![], Json::Null),
            ],
        ),
        complete_desc: "duas janelas: abas fixadas, aba com divisão aninhada e todos os campos, grupo recolhido, janela vazia",
        extras: vec![
            bad(
                "ratio-zero",
                "divisão com ratio 0 (o domínio é 1..65535)",
                workspace_of(tab(2, split(0, 0, pane(3), pane(4)), 3)),
            ),
            bad(
                "ratio-65536",
                "divisão com ratio 65536 (o domínio é 1..65535)",
                workspace_of(tab(2, split(0, 65536, pane(3), pane(4)), 3)),
            ),
            bad(
                "grupo-sem-abas",
                "grupo com a lista de abas vazia (tabs é [+ tab])",
                workspace_of(group(6, "g", 0, false, vec![])),
            ),
        ],
    });

    all.push(Msg {
        folder: "profiles",
        code: Some(11),
        raiz: "message",
        minimal: map(vec![
            ("list", json!([profile("auto:pwsh", "PowerShell", 0)])),
            ("default", json!("auto:pwsh")),
        ]),
        complete: map(vec![
            (
                "list",
                json!([
                    profile("p0", "n", 0),
                    profile("p1", "n", 1),
                    profile("p2", "n", 2),
                    profile("p3", "n", 3),
                ]),
            ),
            ("default", json!("p0")),
        ]),
        complete_desc: "profiles com os quatro tipos de perfil",
        extras: vec![
            ok(
                "lista-vazia",
                "profiles sem nenhum perfil",
                map(vec![("list", json!([])), ("default", json!(""))]),
            ),
            bad(
                "kind-fora-do-dominio",
                "perfil com kind 4, fora do domínio 0..3",
                map(vec![
                    ("list", json!([profile("x", "y", 4)])),
                    ("default", json!("x")),
                ]),
            ),
        ],
    });

    all.push(Msg {
        folder: "theme",
        code: Some(12),
        raiz: "message",
        minimal: theme(0xd7_dce3, 0x15_181d, palette(), json!([0xff_0000_u64])),
        complete: theme(
            0xff_ffff,
            0,
            json!(vec![0xff_ffff_u64; 16]),
            json!([1, 2, 3, 0xab_cdef_u64]),
        ),
        complete_desc: "theme com o maior rgb, o menor rgb e quatro cores de grupo",
        extras: vec![
            bad(
                "rgb-acima-do-maximo",
                "fg 0x1000000, acima de 0xFFFFFF",
                theme(0x100_0000, 0, palette(), json!([1])),
            ),
            bad(
                "paleta-curta",
                "paleta com 15 cores em vez de 16",
                theme(0, 0, json!((0..15).collect::<Vec<u64>>()), json!([1])),
            ),
            bad(
                "sem-cores-de-grupo",
                "group_colors vazio ([+ rgb] pede ao menos uma)",
                theme(0, 0, palette(), json!([])),
            ),
        ],
    });

    // ---- grid ------------------------------------------------------------
    let by_pane = |id: u64| map(vec![("pane", json!(id))]);
    all.push(Msg {
        folder: "subscribe",
        code: Some(20),
        raiz: "message",
        minimal: by_pane(7),
        complete: by_pane(MAX),
        complete_desc: "subscribe do painel de maior id",
        extras: vec![],
    });
    all.push(Msg {
        folder: "unsubscribe",
        code: Some(21),
        raiz: "message",
        minimal: by_pane(7),
        complete: by_pane(MAX),
        complete_desc: "unsubscribe do painel de maior id",
        extras: vec![],
    });

    let snapshot_family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
    all.push(Msg {
        folder: "grid-snapshot",
        code: Some(22),
        raiz: "message",
        minimal: grid(
            Grid::Snapshot,
            2,
            vec![plain()],
            vec![line(false, vec![span(0, 1, 0, "ab")])],
        ),
        complete: grid(
            Grid::Snapshot,
            8,
            vec![
                plain(),
                style(json!([0, 0]), json!([1, 0xff_ffff_u64]), 0x7f),
                style(json!([0, 255]), json!([1, 0]), 0),
            ],
            vec![line(
                true,
                vec![
                    span(0, 2, 1, "界"),
                    gspan(2, 2, 0, snapshot_family),
                    gspan(4, 1, 2, "e\u{301}"),
                    span(5, 1, 0, "abc"),
                ],
            )],
        ),
        complete_desc: "snapshot de 8 colunas com três estilos, linha com quebra suave e quatro spans (largura dupla, família ZWJ, e + acento)",
        extras: {
            let mut extras = grid_extras(Grid::Snapshot);
            extras.push(ok(
                "painel-vazio",
                "snapshot de um painel 0x0: sem estilos e sem linhas",
                grid(Grid::Snapshot, 0, vec![], vec![]),
            ));
            extras
        },
    });

    all.push(Msg {
        folder: "grid-diff",
        code: Some(23),
        raiz: "message",
        minimal: grid(Grid::Diff, 0, vec![], vec![]),
        complete: grid(
            Grid::Diff,
            0,
            vec![style(json!([0, 196]), Json::Null, 1)],
            vec![line(true, vec![span(0, 2, 0, "界")])],
        ),
        complete_desc: "diff com um estilo (vermelho indexado, negrito) e uma linha contínua com 界",
        extras: grid_extras(Grid::Diff),
    });

    all.push(Msg {
        folder: "grid-ack",
        code: Some(24),
        raiz: "message",
        minimal: map(vec![("pane", json!(7)), ("seq", json!(3))]),
        complete: map(vec![("pane", json!(MAX)), ("seq", json!(MAX))]),
        complete_desc: "grid-ack com pane e seq no maior inteiro",
        extras: vec![],
    });

    all.push(Msg {
        folder: "scrollback-request",
        code: Some(25),
        raiz: "message",
        minimal: map(vec![
            ("id", json!(1)),
            ("pane", json!(7)),
            ("offset", json!(0)),
            ("count", json!(1)),
        ]),
        complete: map(vec![
            ("id", json!(MAX)),
            ("pane", json!(7)),
            ("offset", json!(MAX)),
            ("count", json!(500)),
        ]),
        complete_desc: "scrollback-request com id e offset no maior inteiro e count no teto de 500",
        extras: vec![bad(
            "count-acima-de-500",
            "count 501, acima do teto de 500",
            map(vec![
                ("id", json!(1)),
                ("pane", json!(7)),
                ("offset", json!(0)),
                ("count", json!(501)),
            ]),
        )],
    });

    all.push(Msg {
        folder: "scrollback-page",
        code: Some(26),
        raiz: "message",
        // The bytes the Rust and TypeScript codec tests share.
        minimal: map(vec![
            ("id", json!(1)),
            ("pane", json!(7)),
            ("history", json!(5)),
            ("dropped", json!(0)),
            ("offset", json!(0)),
            ("styles", json!([])),
            ("lines", json!([line(false, vec![])])),
        ]),
        complete: grid(
            Grid::Page,
            0,
            vec![style(json!([1, 0x11_2233_u64]), json!([0, 196]), 5)],
            vec![line(true, vec![span(0, 1, 0, "x")]), line(false, vec![])],
        ),
        complete_desc: "página com um estilo (rgb e indexado, negrito e sublinhado) e duas linhas",
        extras: grid_extras(Grid::Page),
    });

    all.push(Msg {
        folder: "subscription-ended",
        code: Some(27),
        raiz: "message",
        minimal: map(vec![("pane", json!(7)), ("reason", json!(0))]),
        complete: map(vec![("pane", json!(MAX)), ("reason", json!(1))]),
        complete_desc: "subscription-ended por host-limit, painel de maior id",
        extras: vec![bad(
            "reason-fora-do-dominio",
            "reason 2, fora do domínio 0..1",
            map(vec![("pane", json!(7)), ("reason", json!(2))]),
        )],
    });

    // ---- input -----------------------------------------------------------
    all.push(Msg {
        folder: "key",
        code: Some(30),
        raiz: "message",
        minimal: map(vec![
            ("pane", json!(7)),
            ("key", json!(0)),
            ("mods", json!(0)),
        ]),
        complete: map(vec![
            ("pane", json!(1)),
            ("key", json!("\u{1F600}")),
            ("mods", json!(15)),
        ]),
        complete_desc: "tecla de um escalar Unicode fora do BMP com as quatro modificadoras",
        extras: vec![
            ok(
                "ctrl-c",
                "o caractere c com Ctrl",
                map(vec![
                    ("pane", json!(7)),
                    ("key", json!("c")),
                    ("mods", json!(1)),
                ]),
            ),
            ok(
                "f12",
                "a tecla nomeada f12 (32), a última do enum",
                map(vec![
                    ("pane", json!(7)),
                    ("key", json!(32)),
                    ("mods", json!(0)),
                ]),
            ),
            bad(
                "tecla-nomeada-fora-do-dominio",
                "número 14, que não é uma tecla nomeada",
                map(vec![
                    ("pane", json!(7)),
                    ("key", json!(14)),
                    ("mods", json!(0)),
                ]),
            ),
        ],
    });
    all.push(Msg {
        folder: "text-input",
        code: Some(31),
        raiz: "message",
        minimal: map(vec![("pane", json!(7)), ("text", json!("a"))]),
        complete: map(vec![
            ("pane", json!(MAX)),
            ("text", json!("ação \u{1F600} e\u{301}")),
        ]),
        complete_desc: "texto com acentos, emoji e acento combinante",
        extras: vec![],
    });
    all.push(Msg {
        folder: "paste",
        code: Some(32),
        raiz: "message",
        minimal: map(vec![("pane", json!(7)), ("text", json!("x"))]),
        complete: map(vec![
            ("pane", json!(MAX)),
            ("text", json!("linha 1\nlinha 2\r\n\t\"aspas\" e \\")),
        ]),
        complete_desc: "colagem de várias linhas com CRLF, tabulação, aspas e barra invertida",
        extras: vec![],
    });

    // ---- actions ---------------------------------------------------------
    let act =
        |id: u64, code: u64, body: Json| map(vec![("id", json!(id)), ("do", json!([code, body]))]);
    all.push(Msg {
        folder: "action",
        code: Some(40),
        raiz: "message",
        minimal: act(1, 0, map(vec![("window", json!(1))])),
        complete: act(
            MAX,
            0,
            map(vec![
                ("window", json!(1)),
                ("group", json!(6)),
                ("profile", json!("auto:pwsh")),
            ]),
        ),
        complete_desc: "new-tab com group e profile",
        extras: vec![
            ok(
                "close-tab",
                "close-tab (1)",
                act(2, 1, map(vec![("tab", json!(2))])),
            ),
            ok(
                "close-pane",
                "close-pane (2)",
                act(3, 2, map(vec![("pane", json!(3))])),
            ),
            ok(
                "rename-tab",
                "rename-tab (3) com um título",
                act(
                    4,
                    3,
                    map(vec![("tab", json!(2)), ("title", json!("build"))]),
                ),
            ),
            ok(
                "rename-tab-limpa",
                "rename-tab (3) com title null: limpa o título próprio",
                act(5, 3, map(vec![("tab", json!(2)), ("title", Json::Null)])),
            ),
            ok(
                "move-tab-to-group",
                "move-tab-to-group (4) para o grupo 6",
                act(6, 4, map(vec![("tab", json!(2)), ("group", json!(6))])),
            ),
            ok(
                "move-tab-para-fora-do-grupo",
                "move-tab-to-group (4) com group null: tira a aba do grupo",
                act(7, 4, map(vec![("tab", json!(2)), ("group", Json::Null)])),
            ),
            ok(
                "move-tab-to-window",
                "move-tab-to-window (5)",
                act(8, 5, map(vec![("tab", json!(2)), ("window", json!(2))])),
            ),
            ok(
                "new-group",
                "new-group (6) só com tabs e name",
                act(
                    9,
                    6,
                    map(vec![("tabs", json!([2, 3])), ("name", json!("work"))]),
                ),
            ),
            ok(
                "new-group-com-cor",
                "new-group (6) com color",
                act(
                    10,
                    6,
                    map(vec![
                        ("tabs", json!([2])),
                        ("name", json!("work")),
                        ("color", json!(2)),
                    ]),
                ),
            ),
            ok(
                "rename-group",
                "rename-group (7)",
                act(
                    11,
                    7,
                    map(vec![("group", json!(6)), ("name", json!("Trabalho"))]),
                ),
            ),
            ok(
                "set-group-color",
                "set-group-color (8)",
                act(12, 8, map(vec![("group", json!(6)), ("color", json!(3))])),
            ),
            ok(
                "collapse-group",
                "collapse-group (9)",
                act(
                    13,
                    9,
                    map(vec![("group", json!(6)), ("collapsed", json!(true))]),
                ),
            ),
            ok(
                "move-group-to-window",
                "move-group-to-window (10)",
                act(14, 10, map(vec![("group", json!(6)), ("window", json!(2))])),
            ),
            ok(
                "ungroup",
                "ungroup (11)",
                act(15, 11, map(vec![("group", json!(6))])),
            ),
            ok(
                "start-pane",
                "start-pane (12)",
                act(16, 12, map(vec![("pane", json!(3))])),
            ),
            ok(
                "activate-tab",
                "activate-tab (13)",
                act(17, 13, map(vec![("tab", json!(5))])),
            ),
        ],
    });
    all.push(Msg {
        folder: "action-result",
        code: Some(41),
        raiz: "message",
        minimal: map(vec![("id", json!(1))]),
        complete: map(vec![
            ("id", json!(MAX)),
            ("error", json!(5)),
            ("created", json!(3)),
        ]),
        complete_desc: "action-result com error e created, as duas chaves opcionais",
        extras: vec![
            ok(
                "criou",
                "sucesso que criou algo (created)",
                map(vec![("id", json!(1)), ("created", json!(9))]),
            ),
            ok(
                "erro",
                "falha com error unknown-tab",
                map(vec![("id", json!(1)), ("error", json!(5))]),
            ),
        ],
    });
    all.push(Msg {
        folder: "confirm-request",
        code: Some(42),
        raiz: "message",
        minimal: map(vec![
            ("id", json!(1)),
            ("kind", json!(0)),
            ("processes", json!([])),
        ]),
        complete: map(vec![
            ("id", json!(MAX)),
            ("kind", json!(1)),
            ("processes", json!(["pwsh", "ssh", "vim \u{e9}"])),
        ]),
        complete_desc: "confirm-request de conexão SSH com três nomes de processo",
        extras: vec![],
    });
    all.push(Msg {
        folder: "confirm",
        code: Some(43),
        raiz: "message",
        minimal: map(vec![("id", json!(1)), ("accept", json!(true))]),
        complete: map(vec![("id", json!(MAX)), ("accept", json!(false))]),
        complete_desc: "confirm recusando, com id no maior inteiro",
        extras: vec![],
    });

    // ---- the rest --------------------------------------------------------
    all.push(Msg {
        folder: "bell",
        code: Some(50),
        raiz: "message",
        minimal: map(vec![("pane", json!(7))]),
        complete: map(vec![("pane", json!(MAX))]),
        complete_desc: "bell do painel de maior id",
        extras: vec![],
    });
    all.push(Msg {
        folder: "pair-accepted",
        code: Some(60),
        raiz: "message",
        minimal: map(vec![("host_name", json!("desk"))]),
        complete: map(vec![("host_name", json!("Mesa do Léo (casa)"))]),
        complete_desc: "pair-accepted com nome fora do ASCII",
        extras: vec![],
    });
    all.push(Msg {
        folder: "pair-rejected",
        code: Some(61),
        raiz: "message",
        minimal: map(vec![("reason", json!(0))]),
        complete: map(vec![("reason", json!(2))]),
        complete_desc: "pair-rejected por timeout, o último valor do enum",
        extras: vec![ok(
            "user-declined",
            "pair-rejected recusado pelo usuário",
            map(vec![("reason", json!(1))]),
        )],
    });
    all.push(Msg {
        folder: "error",
        code: Some(70),
        raiz: "message",
        minimal: map(vec![("code", json!(0))]),
        complete: map(vec![
            ("code", json!(5)),
            ("id", json!(7)),
            ("pane", json!(3)),
        ]),
        complete_desc: "error unknown-tab com id e pane, as duas chaves opcionais",
        extras: vec![
            ok(
                "com-id",
                "error com o id do pedido (resposta a um scrollback-request)",
                map(vec![("code", json!(5)), ("id", json!(7))]),
            ),
            ok(
                "com-pane",
                "error com o painel (resposta a um subscribe)",
                map(vec![("code", json!(9)), ("pane", json!(3))]),
            ),
            ok(
                "last-tab",
                "error last-tab (16), o último código",
                map(vec![("code", json!(16))]),
            ),
            bad(
                "codigo-reservado",
                "error com code 2, reservado (a revogação é goodbye)",
                map(vec![("code", json!(2))]),
            ),
        ],
    });

    // ---- pair-request (a bare map, no type number) -----------------------
    all.push(Msg {
        folder: "pair-request",
        code: None,
        raiz: "pair-request",
        minimal: map(vec![
            ("token", bytes(&[0u8; 16])),
            ("device", json!("a")),
            ("platform", json!(2)),
        ]),
        complete: map(vec![
            ("token", bytes(&[0xff; 16])),
            ("device", json!("Pixel 8 do L\u{e9}o")),
            ("platform", json!(0)),
        ]),
        complete_desc: "pair-request de um aparelho Android com nome fora do ASCII",
        extras: vec![
            ok(
                "ios",
                "pair-request de um aparelho iOS",
                map(vec![
                    ("token", bytes(&seq(0, 16))),
                    ("device", json!("iPhone")),
                    ("platform", json!(1)),
                ]),
            ),
            ok(
                "device-64-bytes",
                "nome do aparelho com exatamente 64 bytes, o máximo",
                map(vec![
                    ("token", bytes(&seq(0, 16))),
                    ("device", json!("d".repeat(64))),
                    ("platform", json!(2)),
                ]),
            ),
            bad(
                "device-vazio",
                "nome do aparelho vazio (o mínimo é 1 byte)",
                map(vec![
                    ("token", bytes(&seq(0, 16))),
                    ("device", json!("")),
                    ("platform", json!(2)),
                ]),
            ),
            bad(
                "device-65-bytes",
                "nome do aparelho com 65 bytes (o máximo é 64)",
                map(vec![
                    ("token", bytes(&seq(0, 16))),
                    ("device", json!("d".repeat(65))),
                    ("platform", json!(2)),
                ]),
            ),
            bad(
                "token-curto",
                "token de 15 bytes em vez de 16",
                map(vec![
                    ("token", bytes(&seq(0, 15))),
                    ("device", json!("a")),
                    ("platform", json!(2)),
                ]),
            ),
            bad(
                "platform-fora-do-dominio",
                "platform 3, fora do domínio 0..2",
                map(vec![
                    ("token", bytes(&seq(0, 16))),
                    ("device", json!("a")),
                    ("platform", json!(3)),
                ]),
            ),
        ],
    });

    all
}

// ---------------------------------------------------------------------------
// Generation
// ---------------------------------------------------------------------------

/// Wraps a body in the message array (`[type, body]`), or returns it as is
/// for the `pair-request`.
fn wrap(code: Option<u64>, body: &Json) -> Json {
    match code {
        Some(code) => json!([code, body]),
        None => body.clone(),
    }
}

/// The wrapped item as a value with its body entries in canonical order.
fn canonical_entries(body: &Json) -> Vec<(String, Value)> {
    let bytes = cbor::encode(&to_value(body));
    match cbor::decode(&bytes).expect("canonical bytes decode") {
        Value::Map(entries) => entries,
        other => panic!("body is not a map: {other:?}"),
    }
}

/// The item for `entries` as a [`Value`] (array of type and body, or the
/// bare map), in the order of `entries`.
fn item_value(code: Option<u64>, entries: Vec<(String, Value)>) -> Value {
    let body = Value::Map(entries);
    match code {
        Some(code) => Value::Array(vec![Value::int(code as i64).expect("type fits"), body]),
        None => body,
    }
}

/// Bytes of the item with the entries in exactly the order given (no
/// sorting): this is how a non-canonical item is built.
fn assemble(code: Option<u64>, entries: &[(String, Value)]) -> Vec<u8> {
    let mut out = Vec::new();
    if let Some(code) = code {
        out.push(0x82);
        out.extend(cbor::encode(&Value::int(code as i64).expect("type fits")));
    }
    assert!(entries.len() < 24, "map head would need more than one byte");
    out.push(0xa0 | entries.len() as u8);
    for (key, value) in entries {
        out.extend(cbor::encode(&Value::Text(key.clone())));
        out.extend(cbor::encode(value));
    }
    out
}

fn file_name(key: &str) -> String {
    key.replace('_', "-")
}

/// All message vectors.
pub fn all() -> Vec<Generated> {
    let mut out = Vec::new();
    for msg in messages() {
        out.extend(cases_of(&msg));
    }
    out
}

fn cases_of(msg: &Msg) -> Vec<Generated> {
    let dir = format!("mensagens/{}", msg.folder);
    let path = |file: &str| format!("{dir}/{file}.json");
    let name = msg.folder;
    let mut out = Vec::new();

    // (1) minimal and (2) complete.
    let minimal = valid(
        &format!("{name} mínimo: só as chaves obrigatórias"),
        msg.raiz,
        &wrap(msg.code, &msg.minimal),
    );
    let minimal_hex = minimal.cbor_hex.clone();
    out.push(Generated::value(path("minimo"), minimal));
    out.push(Generated::value(
        path("completo"),
        valid(
            &format!("{name} completo: {}", msg.complete_desc),
            msg.raiz,
            &wrap(msg.code, &msg.complete),
        ),
    ));

    // Extras.
    for extra in &msg.extras {
        let item = wrap(msg.code, &extra.body);
        let description = format!("{name}: {}", extra.desc);
        let vector = if extra.valid {
            valid(&description, msg.raiz, &item)
        } else {
            let bytes = cbor::encode(&to_value(&item));
            let tree = cbor::decode(&bytes).expect("canonical bytes decode");
            rejected(
                &description,
                msg.raiz,
                Some(&tree),
                &bytes,
                Layer::Mensagem,
                "protocol",
            )
        };
        out.push(Generated::value(path(&extra.file), vector));
    }

    let entries = canonical_entries(&msg.minimal);

    // (3) unknown key, accepted; the typed re-encoding drops it.
    let mut with_unknown = entries.clone();
    with_unknown.push(("zz".to_owned(), Value::Bool(true)));
    let sorted = cbor::encode(&item_value(msg.code, with_unknown));
    let sorted_tree = cbor::decode(&sorted).expect("canonical bytes decode");
    let mut unknown = valid(
        &format!("{name}: chave desconhecida \"zz\" no corpo, aceita e descartada ao recodificar"),
        msg.raiz,
        &from_value(&sorted_tree),
    );
    unknown.reencode_hex = Some(minimal_hex);
    out.push(Generated::value(path("chave-desconhecida"), unknown));

    // (4) each required key removed in turn.
    for (index, (key, _)) in entries.iter().enumerate() {
        let mut rest = entries.clone();
        rest.remove(index);
        let bytes = cbor::encode(&item_value(msg.code, rest));
        let tree = cbor::decode(&bytes).expect("canonical bytes decode");
        out.push(Generated::value(
            path(&format!("sem-{}", file_name(key))),
            rejected(
                &format!("{name}: sem a chave obrigatória \"{key}\""),
                msg.raiz,
                Some(&tree),
                &bytes,
                Layer::Mensagem,
                "protocol",
            ),
        ));
    }

    // (5) not canonical: two keys of the body in the wrong order. A body of
    // one key gets the unknown key "zz" first, so there is something to swap.
    let mut swapped = if entries.len() >= 2 {
        entries
    } else {
        let mut two = entries;
        two.push(("zz".to_owned(), Value::Bool(true)));
        match cbor::decode(&cbor::encode(&Value::Map(two))).expect("canonical bytes decode") {
            Value::Map(sorted) => sorted,
            other => panic!("not a map: {other:?}"),
        }
    };
    swapped.swap(0, 1);
    let bytes = assemble(msg.code, &swapped);
    let tree = item_value(msg.code, swapped);
    out.push(Generated::value(
        path("nao-canonico"),
        rejected(
            &format!("{name}: duas chaves do corpo fora da ordem canônica"),
            msg.raiz,
            Some(&tree),
            &bytes,
            Layer::Mensagem,
            "protocol",
        ),
    ));
    out
}
