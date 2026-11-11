# Othello trainer

The trainer reads the public WTHOR archive of human tournament games and will
learn the pattern weights used by the engine. The engine does not depend on
this crate.

## Data

The WTHOR files are not committed and the trainer never accesses the network.
Download them once, from the repository root:

```sh
mkdir -p data/wthor
for year in $(seq 1977 2025); do
  curl -fL -o "data/wthor/WTH_${year}.wtb" \
    "https://www.ffothello.org/wthor/base/WTH_${year}.wtb"
done
```

The federation publishes the files at
<https://www.ffothello.org/wthor/base/>. It states no licence, so the project
does not redistribute them. `data/` is git-ignored.

`trainer/wthor.sha256` pins the inputs in `shasum -a 256` format. Verify the
download and replay every game with:

```sh
cargo run --release --manifest-path trainer/Cargo.toml -- check
```

`check` refuses a file whose hash does not match the manifest, or a `.wtb`
file that the manifest does not list. It reports games read, replayed and
rejected, real-score agreement, and the P3 values found.

The full-archive result is recorded in `docs/tuning/wthor_check.log`.

## Labelling

The label job solves every position at 14 or fewer empties and writes one
journal line per game to `data/labels.journal`. It is resumable: re-running
the same command skips completed games.

```sh
cargo run --release --manifest-path trainer/Cargo.toml -- label --jobs 5
```

Use `--games 1000` for the pilot. The full run is started only after the
pilot fit. The journal is not committed.
