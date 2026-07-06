import {
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

const selection: PreferredViewSelection = selectPreferredViews([pathInput, bytesInput], {
  preferenceOrder: "synthetic-2d-first",
})
const rcc: MammogramRecord | null = selection.views.rcc
const lcc: MammogramRecord | null = selection.views.lcc
const rmlo: MammogramRecord | null = selection.views.rmlo
const lmlo: MammogramRecord | null = selection.views.lmlo
rcc?.metadata.mammogramType.toUpperCase()
lcc?.source.toString()
rmlo?.source.toString()
lmlo?.source.toString()

selectPreferredViewsFromDirectory("study", { preferenceOrder: "default", strict: false })
