# mstu-media

Media helpers shared by [mstu](https://github.com/ng-he/mstu) plugins.

```toml
[dependencies]
mstu-media = "0.1"
```

| | |
| --- | --- |
| `codec` | NAL unit types, which of them a decoder may start at, and a track's parameter sets. |
| `nalu` | NAL units, AVCC to Annex-B and back, rewritten in place where the sizes allow. |
| `fourcc` | Four-character codes. |
| `rtp` | Packets, the clock their timestamps are on, and access units out of them for H.264 and H.265. |
| `sdp` | Session descriptions: which track is which, at what clock, with which parameter sets. |
| `base64` | What SDP carries its parameter sets in, and what a login is encoded as. |

A track's `Parameters` hold what it declares up front as the Annex-B blob that
travels with the stream -- the same bytes an mstu `extras` field carries -- and
hand the parts back out on ask:

```rust
let params = h264::Parameters::new(extras);     // nothing taken apart

params.as_bytes();                              // and nothing put back together
params.sps();                                   // shares the bytes, copies none
params.profile();                               // read out of the sps
```

Nothing here knows about the engine, or about any one protocol: RTSP, RTP over
UDP and WebRTC all describe media the same way, and this is that part.

A plugin that parses or rewrites elementary streams can use it; one that does
not, need not.

## Media for tests

The `fixture` feature hands out parameter sets, slices, packets and
descriptions built byte by byte, so a plugin's tests need not write their own
out by hand:

```toml
[dev-dependencies]
mstu-media = { version = "0.1", features = ["fixture"] }
```

## License

MIT or Apache-2.0, at your option.
