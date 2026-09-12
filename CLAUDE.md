# market-rules (nom provisoire)

Moteur d'exécution de règles de marché sandboxées en WebAssembly. Il consomme les
enregistrements produits par market-stream (Go) et exécute sur chaque trade des règles
écrites par l'utilisateur, compilées en modules WASM et isolées dans wasmtime avec un
budget d'instructions (fuel) et une limite mémoire.

## Contexte et référence

- Le repo market-stream est disponible en lecture seule dans `../market-stream`.
  Ne jamais le modifier. S'y référer pour le format `.msr.zst` et la dérivation des
  exposants.
- Fichiers de référence côté Go : `format.go`, `writer.go`, `recorder.go` (format MSR),
  `decode.go` (décodage aggTrade), `exchangeinfo.go:194-203` (exposants).
- Corpus de test : `../market-stream/docs/baseline/reference.msr.zst`, à copier dans
  `fixtures/`.

## Format MSR (entrée principale)

Un seul flux zstd contenant :

1. En-tête : `MSREC\0` puis `u16` LE version, qui doit valoir 1.
2. Enregistrements successifs : `kind u8 | receivedAt i64 ns | length u32 | payload`,
   tout en little-endian, payload de 16 MiB maximum.

| kind | Contenu | Traitement v0.1 |
|------|---------|-----------------|
| 1 frame | JSON Binance brut | décoder `aggTrade`, ignorer `depthUpdate` |
| 2 snapshot | JSON `model.Snapshot` sans tags | ignorer |
| 3 meta | exchangeInfo brut, en tête de fichier | extraire les exposants |
| 4 drop | `u64` LE, trames perdues | compter dans les stats |

Points d'attention :

- Vérifier sur le corpus si les trames `aggTrade` sont enveloppées dans le format
  combined stream (`{"stream":..., "data":{...}}`) et gérer les deux formes.
- Champs aggTrade utiles : `E` (heure d'événement, ms), `s`, `p`, `q`, `m`.
  Côté taker : `is_buy = !m`, comme dans market-stream.
- Symbole normalisé `BASE-QUOTE` (ex. `BTC-USDT`), reconstruit depuis `baseAsset` et
  `quoteAsset` du exchangeInfo.
- Exposants : nombre de décimales significatives de `tickSize` (prix) et `stepSize`
  (quantité). Reproduire exactement la sémantique de `exchangeinfo.go:194-203`.
- Conversion décimale vers virgule fixe : `"104.48000000"` avec exposant 2 donne
  `10448`. Des zéros au-delà de l'exposant sont tolérés, un chiffre non nul au-delà est
  une erreur explicite, jamais un arrondi silencieux.

## Modèle de données

```rust
pub struct Trade {
    pub symbol_id: u32,       // index dans la table des instruments
    pub exchange_ts_ns: i64,  // E de Binance, ms x 1e6
    pub received_ts_ns: i64,  // receivedAt de l'enregistrement MSR
    pub price: i64,           // virgule fixe, exposant de l'instrument
    pub qty: i64,
    pub is_buy: bool,
}

pub struct Instrument {
    pub symbol: String,
    pub price_exp: u8,
    pub qty_exp: u8,
}

pub trait TickSource {
    fn next_trade(&mut self) -> Result<Option<Trade>, SourceError>;
    fn instruments(&self) -> &[Instrument];
}
```

Pas de float dans le chemin chaud, ni côté hôte ni côté règles.

## Architecture du workspace

```
crates/
  host/          lib + binaire : sources, lecteur MSR, moteur wasmtime, CLI
  rule-logic/    logique pure des règles, sans I/O, compilable en natif et en wasm32
rules/
  sma-cross/     cdylib wasm32 : fine couche ABI autour de rule-logic
  volume-spike/  cdylib wasm32
  test-loop/     cdylib de test : boucle infinie (doit être coupée par le fuel)
  test-mem-hog/  cdylib de test : croissance mémoire (doit être refusée)
fixtures/        corpus MSR de référence
benches/         benchmarks criterion
```

`rule-logic` est utilisé à la fois par les crates WASM et directement par l'hôte en
natif, pour que le benchmark natif contre WASM compare exactement le même code.

Build des règles :
`cargo build -p sma-cross --target wasm32-unknown-unknown --release`

## ABI invité v0 (scalaires uniquement)

Une instance par couple (règle, symbole). Exports attendus :

```
init(price_exp: i32, qty_exp: i32) -> i32              // 0 = ok
on_trade(ts_ns: i64, price: i64, qty: i64, is_buy: i32) -> i32
                                                        // 0 = rien, >0 = code signal
```

Pas d'import côté hôte en v0. Le component model et WIT sont une évolution documentée
dans le README, pas un objectif v0.1.

État des règles côté invité : wasm32-unknown-unknown est mono-thread. Utiliser un
`thread_local!` avec `RefCell` plutôt que `static mut` (restrictions de l'édition 2024).

## Moteur wasmtime

- Un `Engine` et un `Module` compilé une seule fois par règle, une instance et un
  `Store` par couple (règle, symbole).
- Récupérer les `TypedFunc` une seule fois et les réutiliser : pas de lookup par appel.
- Fuel activé (`consume_fuel`), budget rechargé avant chaque appel `on_trade`.
- Limite mémoire via `StoreLimits`.
- Un trap (fuel épuisé, mémoire refusée, panic invité) désactive l'instance concernée,
  est compté dans les stats et n'interrompt jamais le traitement des autres règles.
- L'API fuel a changé selon les versions de wasmtime : vérifier la documentation de la
  version épinglée avant d'écrire le code.

## Règles v0.1

- `sma-cross` : moyennes mobiles courte et longue sur les N derniers trades, buffers
  circulaires à taille fixe, aucune allocation après `init`. Signal 1 croisement
  haussier, 2 croisement baissier.
- `volume-spike` : somme glissante des quantités comparée à une moyenne exponentielle,
  signal quand le ratio dépasse un seuil.

## CLI

```
market-rules --source msr:fixtures/reference.msr.zst \
             --rule target/wasm32-unknown-unknown/release/sma_cross.wasm
market-rules --source stdin --rule ...   # pipe depuis ingestd, optionnel
```

Sortie : un signal par ligne en NDJSON sur stdout
(`ts_ns`, `symbol`, `rule`, `signal`). Statistiques finales sur stderr : trades lus,
signaux, traps par règle, trames perdues.

Source stdin (optionnelle) : filtrer les lignes `msg == "trade"` de ingestd. Pas
d'heure d'événement disponible, utiliser `time` comme heure de réception. L'exposant se
déduit du nombre de décimales de la chaîne (`"0.00010"` donne 5).

## Benchmarks (criterion)

1. Lecture MSR plus décodage aggTrade sur le corpus : enregistrements par seconde.
2. `sma-cross` natif contre WASM : ns par appel.
3. Coût du fuel : WASM avec et sans comptage.

Les chiffres du README viennent de la machine locale, avec le matériel indiqué. En CI,
seulement `cargo bench --no-run`.

## Priorités et ordre de coupe

Ordre de réalisation :

1. Conversion virgule fixe, lecteur MSR, décodage aggTrade, tests sur le corpus.
2. Moteur wasmtime et `sma-cross`.
3. Limites fuel et mémoire, tests avec `test-loop` et `test-mem-hog`.
4. Benchmarks.
5. `volume-spike` et source stdin.
6. CI et README.

Si le temps manque, couper dans cet ordre : source stdin, puis `volume-spike`. Les
étapes 1 à 4 ne se coupent pas.

## Conventions de contribution

- Pas de `unsafe`. Pas d'async en v0.1.
- Erreurs : `thiserror` dans les bibliothèques, `anyhow` uniquement dans le binaire.
- Toute nouvelle dépendance est signalée et justifiée avant ajout. Base prévue :
  `wasmtime`, `zstd`, `serde`, `serde_json`, `thiserror`, `anyhow`, `clap`,
  `criterion` (dev).
- `crates/host/src/bars.rs` est un squelette volontaire, hors de `lib.rs`. Ne pas
  implémenter ses `todo!()` ni le brancher dans le pipeline sans demande explicite.
- Pas de cadratins dans la documentation et le README.
- Avant de proposer un commit : `cargo fmt`,
  `cargo clippy --workspace --exclude sma-cross --exclude volume-spike --all-targets -- -D warnings`,
  `cargo test`.
