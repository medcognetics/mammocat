# @medcognetics/mammocat

Node/TypeScript bindings for mammocat mammography DICOM metadata extraction, conventional orientation assessment, and preferred-view selection.

```ts
import {
  assessConventionalOrientation,
  extractMetadata,
  selectPreferredViewsFromDirectory,
} from "@medcognetics/mammocat"

const metadata = extractMetadata({ path: "study/L_CC.dcm" })
const orientation = assessConventionalOrientation({ path: "study/L_CC.dcm" })
const selection = selectPreferredViewsFromDirectory("study")

console.log(metadata.mammogramType)
console.log(metadata.viewModifiers)
console.log(metadata.conventionalOrientation.status)
console.log(orientation.horizontalFlipRequired)
console.log(selection.views.rcc?.source)
```

The public API returns JSON-serializable objects suitable for Electron IPC boundaries. Bulk selection keeps unreadable or unsupported DICOM inputs in `inputErrors` instead of throwing; malformed API arguments still throw.

`viewPosition` uses the complete CID 4014 base-view set. `viewModifiers` contains normalized CID 4015 values such as `implant_displaced` and `spot_compression`. Version 0.2.0 removes AT and CV as base views; they are exposed as `axillary_tail` and `cleavage` modifiers.

`assessConventionalOrientation` accepts the same path-or-bytes input as `extractMetadata`,
but does not require a mammography SOP Class, `MG` modality, or recognized mammogram type.
It compares PatientOrientation with RCC `P\L`, LCC `A\R`, RMLO `P\FL`, or LMLO `A\FR`
and returns `matches`, `requires_flip`, `indeterminate`, or `not_applicable`. Flip flags
are non-null only when both components are exact expected values or complete anatomical
inverses. The assessment does not modify pixels or refresh attributes that could become
stale after a downstream flip.

The npm package loads a platform-specific optional native package at install time. Publish-ready targets are:

- `@medcognetics/mammocat-linux-x64-gnu`
- `@medcognetics/mammocat-darwin-x64`
- `@medcognetics/mammocat-darwin-arm64`
- `@medcognetics/mammocat-win32-x64-msvc`

## Install from a Git commit

The repository root is also an installable source package. Pin the dependency to a full commit SHA:

```json
{
  "dependencies": {
    "@medcognetics/mammocat": "git+https://github.com/medcognetics/mammocat.git#<full-commit-sha>"
  }
}
```

Install and reproduce the locked revision with:

```bash
npm install --omit=optional
npm ci --omit=optional
```

The Git package runs a `prepare` build for the host and installs the generated `.node` file beside the existing JavaScript loader and TypeScript declarations. Building requires Git, Node.js 22 or newer, Rust 1.88 or newer, Cargo, and the native toolchain for Linux x64 GNU, macOS x64, macOS arm64, or Windows x64 MSVC. The consumer lockfile retains the full source commit SHA.
