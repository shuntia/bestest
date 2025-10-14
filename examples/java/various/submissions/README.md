# Java "various" submissions

Drop candidate archives (for example, `alice_1_01_submission.zip`) into this
folder while experimenting with the `examples/java/various/config.toml`
scenario. The configuration intentionally mixes:

- A passing case that should output `5`.
- A logic error producing a runtime exception.
- A run that should hit the harness timeout.
- A crash path that surfaces an unhandled exception message.

The helpers referenced by the config live in `../deps/`.
