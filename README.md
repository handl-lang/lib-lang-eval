# lib-lang-eval

`handl-plugin-lib-lang-eval` — the `eval` meta function as a composable
`plugin-lib-*` leaf crate.

## Layout

```text
handl-packages/lang_eval/lang_eval.hnl   the .hnl surface
src/lib.rs, src/tests.rs                 the Rust crate
scripts/hnl-gate.sh                      the shared hnl typecheck/lint/test gate
ci.sh                                    bupha entry — delegates to hnl-gate.sh
```

Sibling-repo deps are git dependencies (`branch = "main"`); `[patch]`
tables resolve them from sibling checkouts.

## Gate

```bash
cargo test    # the Rust suite
bash ci.sh    # the bupha gate — delegates to scripts/hnl-gate.sh
```

## License

MIT.
