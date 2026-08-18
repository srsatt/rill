# Example static source plugin

`component.wat` is a complete WebAssembly Component Model source plugin. It requests no capabilities and returns one deterministic item, so it is useful for host acceptance tests.

Install through `POST /api/v1/plugins/install`, inspect the returned SHA-256 and manifest, enable it, then create a source instance using its installation ID.
