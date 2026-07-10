# @medcognetics/mammocat

Node/TypeScript bindings for mammocat mammography DICOM metadata extraction and preferred-view selection.

```ts
import { extractMetadata, selectPreferredViewsFromDirectory } from "@medcognetics/mammocat"

const metadata = extractMetadata({ path: "study/L_CC.dcm" })
const selection = selectPreferredViewsFromDirectory("study")

console.log(metadata.mammogramType)
console.log(selection.views.rcc?.source)
```

The public API returns JSON-serializable objects suitable for Electron IPC boundaries. Bulk selection keeps unreadable or unsupported DICOM inputs in `inputErrors` instead of throwing; malformed API arguments still throw.

The npm package loads a platform-specific optional native package at install time. Publish-ready targets are:

- `@medcognetics/mammocat-linux-x64-gnu`
- `@medcognetics/mammocat-darwin-x64`
- `@medcognetics/mammocat-darwin-arm64`
- `@medcognetics/mammocat-win32-x64-msvc`
