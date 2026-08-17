import {
  assessConventionalOrientation,
  extractMetadata,
  selectPreferredViews,
  selectPreferredViewsFromDirectory,
  type DicomInput,
  type MammogramRecord,
  type PreferredViewSelection,
} from "../index"

const pathInput: DicomInput = { path: "study/R_CC.dcm" }
const bytesInput: DicomInput = {
  bytes: new Uint8Array([1, 2, 3]),
  filename: "upload.dcm",
}

const metadata = extractMetadata(pathInput)
metadata.pixelSpacing?.column.toFixed(3)
metadata.viewModifiers.map((modifier) => modifier.toUpperCase())
metadata.conventionalOrientation.status.toUpperCase()

const orientation = assessConventionalOrientation(bytesInput)
orientation.expectedComponents?.map((component) => component.toUpperCase())

const selection: PreferredViewSelection = selectPreferredViews([pathInput, bytesInput], {
  preferenceOrder: "synthetic-2d-first",
  viewFallbackPolicy: { mode: "allow-list", allowedViews: ["ml", "xccl"] },
  viewModifierPolicy: {
    mode: "allow-list",
    allowedModifiers: ["implant-displaced", "spot-compression"],
  },
})
const rcc: MammogramRecord | null = selection.views.rcc
const lcc: MammogramRecord | null = selection.views.lcc
const rmlo: MammogramRecord | null = selection.views.rmlo
const lmlo: MammogramRecord | null = selection.views.lmlo
rcc?.metadata.mammogramType.toUpperCase()
lcc?.source.toString()
rmlo?.source.toString()
lmlo?.source.toString()

selectPreferredViewsFromDirectory("study", {
  preferenceOrder: "default",
  strict: false,
  viewFallbackPolicy: { mode: "standard-only" },
  viewModifierPolicy: { mode: "unmodified-only" },
})

selectPreferredViews([], {
  // @ts-expect-error allow-list fallback policies require allowedViews
  viewFallbackPolicy: { mode: "allow-list" },
})
selectPreferredViews([], {
  // @ts-expect-error non-allow-list fallback policies forbid allowedViews
  viewFallbackPolicy: { mode: "standard-only", allowedViews: ["ml"] },
})
selectPreferredViews([], {
  // @ts-expect-error allow-list modifier policies require allowedModifiers
  viewModifierPolicy: { mode: "allow-list" },
})
selectPreferredViews([], {
  // @ts-expect-error non-allow-list modifier policies forbid allowedModifiers
  viewModifierPolicy: { mode: "unmodified-only", allowedModifiers: ["magnification"] },
})
