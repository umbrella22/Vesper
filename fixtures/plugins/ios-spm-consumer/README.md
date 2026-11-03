# SwiftPM Release Consumer

This fixture resolves `VesperPlayerKit` from the dedicated remote Swift Package
repository at the published `0.9.1` version. It deliberately has no local
package path, Rust resolver bundle, or repository source dependency.

Run package resolution from the repository root:

```sh
swift package resolve --package-path fixtures/plugins/ios-spm-consumer
```

An iOS host project should link the resolved `VesperPlayerKit` product and use
the same controller/source construction shown in `Sources/VesperIosSpmConsumer`.
Building or running a simulator/device host is a separate platform check; this
fixture proves that the published SwiftPM manifest, binary artifacts, and
versioned product can be resolved without a checkout-local Rust resolver bundle.
