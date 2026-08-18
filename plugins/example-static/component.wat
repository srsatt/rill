(component
  (import "rill:source-plugin/host@1.0.0"
    (instance $host
      (export "log" (func (param "level" string) (param "message" string)))
      (export "get-secret" (func (param "name" string) (result (result string (error string)))))
      (export "http-get" (func (param "url" string) (param "maximum-bytes" u32) (result (result string (error string)))))
    )
  )

  (core module $implementation
    (memory $memory 2)
    (global $next (mut i32) (i32.const 4096))
    (data (i32.const 1024) "{\"id\":\"example-static\",\"name\":\"Example Static\",\"version\":\"1.0.0\",\"description\":\"Returns one deterministic story without network or secrets.\",\"requestedPermissions\":[]}")
    (data (i32.const 1216) "{\"type\":\"object\",\"additionalProperties\":false}")
    (data (i32.const 1280) "{}")
    (data (i32.const 1344) "{\"items\":[{\"externalId\":\"welcome\",\"itemKind\":\"article\",\"title\":\"Welcome from a Component\",\"bodyText\":\"This item came from the example Rill WebAssembly Component Model source plugin.\",\"bodyHtml\":null,\"author\":\"Rill example\",\"sourceUrl\":\"https://example.com/rill-component\",\"publishedAt\":null,\"metadata\":{\"example\":true}}],\"cursor\":{\"page\":1},\"notModified\":false}")

    (func $realloc (param $old i32) (param $old-size i32) (param $align i32) (param $new-size i32) (result i32)
      (local $result i32)
      global.get $next
      local.set $result
      global.get $next
      local.get $new-size
      i32.add
      i32.const 7
      i32.add
      i32.const -8
      i32.and
      global.set $next
      local.get $result)

    (func $metadata (result i32)
      i32.const 0 i32.const 1024 i32.store
      i32.const 4 i32.const 167 i32.store
      i32.const 0)
    (func $config-schema (result i32)
      i32.const 8 i32.const 1216 i32.store
      i32.const 12 i32.const 46 i32.store
      i32.const 8)
    (func $validate-config (param i32 i32) (result i32)
      i32.const 16 i32.const 0 i32.store
      i32.const 20 i32.const 1280 i32.store
      i32.const 24 i32.const 2 i32.store
      i32.const 16)
    (func $poll (param i32 i32 i32 i32 i32 i32) (result i32)
      i32.const 32 i32.const 0 i32.store
      i32.const 36 i32.const 1344 i32.store
      i32.const 40 i32.const 361 i32.store
      i32.const 32)

    (export "memory" (memory $memory))
    (export "cabi_realloc" (func $realloc))
    (export "metadata" (func $metadata))
    (export "config-schema" (func $config-schema))
    (export "validate-config" (func $validate-config))
    (export "poll" (func $poll))
  )

  (core instance $implementation-instance (instantiate $implementation))
  (alias core export $implementation-instance "memory" (core memory $memory))
  (alias core export $implementation-instance "cabi_realloc" (core func $realloc))
  (alias core export $implementation-instance "metadata" (core func $metadata))
  (alias core export $implementation-instance "config-schema" (core func $config-schema))
  (alias core export $implementation-instance "validate-config" (core func $validate-config))
  (alias core export $implementation-instance "poll" (core func $poll))

  (type $metadata-type (func (result string)))
  (type $config-schema-type (func (result string)))
  (type $validate-config-type (func (param "config-json" string) (result (result string (error string)))))
  (type $poll-type (func (param "config-json" string) (param "cursor-json" (option string)) (param "limit" u32) (result (result string (error string)))))

  (func $metadata-lifted (type $metadata-type)
    (canon lift (core func $metadata) (memory $memory) (realloc $realloc) string-encoding=utf8))
  (func $config-schema-lifted (type $config-schema-type)
    (canon lift (core func $config-schema) (memory $memory) (realloc $realloc) string-encoding=utf8))
  (func $validate-config-lifted (type $validate-config-type)
    (canon lift (core func $validate-config) (memory $memory) (realloc $realloc) string-encoding=utf8))
  (func $poll-lifted (type $poll-type)
    (canon lift (core func $poll) (memory $memory) (realloc $realloc) string-encoding=utf8))

  (instance $source
    (export "metadata" (func $metadata-lifted))
    (export "config-schema" (func $config-schema-lifted))
    (export "validate-config" (func $validate-config-lifted))
    (export "poll" (func $poll-lifted))
  )
  (export "rill:source-plugin/source@1.0.0" (instance $source))
)
