# symphonia is pinned by songbird

Why dependabot's symphonia 0.6 pull requests cannot be merged, what would have
to happen first, and the trap in our own `Cargo.toml` that is easy to trip while
tidying up.

Verified 2026-08-27 against upstream, the lockfile, and both crates' docs.

## The short version

`symphonia`'s version is not ours to choose. `songbird` pins `^0.5.2`, our
direct dependency exists to unify with it, and the two must move together or
nothing compiles.

## Why the bump fails

Dependabot's symphonia 0.6.1 PR fails to build with:

```
error[E0053]: method `create` has an incompatible type for trait
  --> crack-core/src/sources/rusty_ytdl.rs:186:5
  --> symphonia-core-0.5.5/src/io/mod.rs:42:1
error[E0053]: method `create_async` has an incompatible type for trait
```

Read which version the *trait* comes from: `symphonia-core-**0.5.5**`, while the
PR moves our direct dependency to 0.6.1.

This is trait **identity**, not a trait change. `MediaSource` is byte-identical
in both versions:

```rust
pub trait MediaSource: Read + Seek + Send + Sync {
    fn is_seekable(&self) -> bool;
    fn byte_len(&self) -> Option<u64>;
}
```

`crack-core/src/sources/rusty_ytdl.rs` implements that trait and hands the result
to songbird's `Compose`, whose signature is
`Result<AudioStream<Box<dyn MediaSource>>, AudioStreamError>`. Bump only our
side and two `symphonia-core` crates land in one graph — our impl satisfies
0.6's trait, songbird wants 0.5's, and to rustc those are two different traits
that merely look alike.

**Our own porting cost for symphonia 0.6 is zero.** The workspace's entire
symphonia surface is one line:

```
crack-core/src/sources/rusty_ytdl.rs:21:  use symphonia::core::io::MediaSource;
```

## Who else is involved — nobody

Only two crates in the whole lock graph depend on symphonia: `crack-core` and
`songbird`. Not serenity, not poise, not lavalink-rs, not rusty_ytdl. There is no
wide multi-crate bump to coordinate. The cascade is narrow and deep, not broad.

## songbird pins 0.5.2 on every branch

Checked against `serenity-rs/songbird` directly:

| branch | symphonia |
|---|---|
| `current` | `0.5.2` |
| `next` | `0.5.2` |
| `serenity-next` — the branch we track | `0.5.2` |
| `v0.4.x` | `0.5.2` |

No open issue or PR proposes 0.6; the only symphonia issues are closed ones from
the original 0.5 migration. Our own fork (`CycleFive/songbird`, branch
`bot-template-experiment`) sits on top of `serenity-next` and is likewise on
0.5.2. There is no branch to switch to.

## What the songbird port would involve

symphonia 0.6 restructured precisely the surfaces songbird is built on. Its
exposure, across 34 files:

| symphonia API | songbird refs | what 0.6 did |
|---|---:|---|
| `AudioBuffer` | 63 | `Signal` split into `Audio`, `AudioMut`, `AudioBytes`, `AudioBufferBytes` |
| `AudioBufferRef` | 42 | replaced by `GenericAudioBufferRef`, no longer copy-on-write |
| `Layout` | 16 | **removed**; replaced by a `layouts` submodule of `Channels` constants |
| `CodecRegistry` | 15 | `register()` **removed**; `make()` → `make_audio_decoder()` |
| `Probe` / `get_probe` | 15 / 14 | moved to `formats::probe`; `format()` → `probe()`; tiered registration |
| `SignalSpec` | 13 | renamed `AudioSpec`; `Channels` no longer a bitmask |
| `MediaSourceStream` | 11 | gained an explicit lifetime |
| `next_packet` | 6 | returns `Ok(None)` at EOF instead of an error |
| `SampleBuffer` | 5 | **removed** — use the new trait copy functions |
| `QueryDescriptor` | 4 | **removed** — implement `ProbeableFormat` instead |

Two items make this more than a mechanical port:

1. **It breaks songbird's public API.** `Config` exposes
   `codec_registry: &'static CodecRegistry` and `format_registry: &'static Probe`.
   Both types moved and changed shape, so a bump is semver-breaking for every
   songbird consumer.
2. **songbird registers its own codecs.** It ships `FormatReader` impls for dca
   and raw plus an `OpusDecoder`, registered through `CodecRegistry::register()`
   — the exact call 0.6 removed.

symphonia 0.6 also raises MSRV to 1.85. That part is free for us: no
`rust-version` pin anywhere, and `rust-toolchain` is `stable`.

## 🪤 The trap — do not "clean up" our direct dependency

songbird re-exports symphonia at `songbird::input::core`
(`pub use symphonia_core as core;`), which makes an obvious-looking tidy-up
suggest itself: drop our direct symphonia dependency and import `MediaSource`
from songbird instead, so only one symphonia can ever be in the graph.

**That would silently break audio playback.** Compare the declarations:

```toml
# ours -- Cargo.toml
symphonia = { version = "0.5.4", features = ["all-formats", "all-codecs", "opt-simd"] }

# songbird's
symphonia = { default-features = false, optional = true, version = "0.5.2" }
```

songbird enables **no format or codec features at all**. Our declaration is what
actually switches on MP3, AAC, FLAC, Vorbis, MKV and ISO-MP4 decoding, via cargo
feature unification. Remove it and the bot keeps compiling while losing the
ability to decode nearly everything.

If that consolidation is ever wanted, the codec features have to move onto
songbird's side first.

## Loose end

`[workspace.dependencies.symphonia-metadata]` in the root `Cargo.toml` is
referenced by no crate and no source file. It is dead and can be deleted on its
own; it is left in place here only because removing it is unrelated to this
document.
