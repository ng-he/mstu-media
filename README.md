# mstu-media

Media helpers shared by [mstu](https://github.com/ng-he/mstu) plugins: codec
parameters, four-character codes, and NAL unit handling including AVCC to
Annex-B rewriting in place.

```toml
[dependencies]
mstu-media = "0.1"
```

Nothing here knows about the engine. A plugin that parses or rewrites elementary
streams can use it; one that does not, need not.

## License

MIT or Apache-2.0, at your option.
