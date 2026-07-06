import assert from "node:assert/strict"
import { mkdir, mkdtemp, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { test } from "node:test"

import {
  extractMetadata,
  selectPreferredViews,
  selectPreferredViewsFromDirectory,
} from "../index.js"
import { createMammogramBytes, writeMammogramFile } from "./dicom-fixtures.mjs"

test("extractMetadata returns JSON-safe metadata from a file path", async (t) => {
  const directory = await tempDir()
  const path = await writeMammogramFile(directory, "l_mlo.dcm", {
    laterality: "L",
    viewPosition: "MLO",
  })

  const metadata = extractMetadata({ path })

  assert.equal(metadata.mammogramType, "ffdm")
  assert.equal(metadata.laterality, "left")
  assert.equal(metadata.viewPosition, "mlo")
  assert.deepEqual(metadata.pixelSpacing, { row: 0.07, column: 0.07 })
  assert.doesNotThrow(() => JSON.stringify(metadata))
})

test("file path and byte inputs produce matching metadata", async (t) => {
  const directory = await tempDir()
  const path = await writeMammogramFile(directory, "r_cc.dcm", {
    laterality: "R",
    viewPosition: "CC",
  })
  const bytes = createMammogramBytes({ laterality: "R", viewPosition: "CC" })

  const fromFile = extractMetadata({ path })
  const fromBytes = extractMetadata({ bytes, filename: "r_cc_upload.dcm" })

  assert.equal(fromBytes.mammogramType, fromFile.mammogramType)
  assert.equal(fromBytes.laterality, fromFile.laterality)
  assert.equal(fromBytes.viewPosition, fromFile.viewPosition)
  assert.deepEqual(fromBytes.pixelSpacing, fromFile.pixelSpacing)
})

test("selectPreferredViews returns the four preferred standard slots", () => {
  const inputs = [
    { bytes: createMammogramBytes({ laterality: "R", viewPosition: "CC" }), filename: "rcc.dcm" },
    { bytes: createMammogramBytes({ laterality: "L", viewPosition: "CC" }), filename: "lcc.dcm" },
    { bytes: createMammogramBytes({ laterality: "R", viewPosition: "MLO" }), filename: "rmlo.dcm" },
    { bytes: createMammogramBytes({ laterality: "L", viewPosition: "MLO" }), filename: "lmlo.dcm" },
  ]

  const selection = selectPreferredViews(inputs)

  assert.equal(selection.views.rcc?.source, "rcc.dcm")
  assert.equal(selection.views.lcc?.source, "lcc.dcm")
  assert.equal(selection.views.rmlo?.source, "rmlo.dcm")
  assert.equal(selection.views.lmlo?.source, "lmlo.dcm")
  assert.deepEqual(selection.missingViews, [])
  assert.deepEqual(selection.inputErrors, [])
  assert.doesNotThrow(() => JSON.stringify(selection))
})

test("duplicate candidates choose the larger image for the same view", () => {
  const selection = selectPreferredViews([
    {
      bytes: createMammogramBytes({
        laterality: "R",
        viewPosition: "CC",
        rows: 1024,
        columns: 1024,
        sopInstanceUid: "1.2.826.0.1.3680043.10.543.10.1",
      }),
      filename: "small_rcc.dcm",
    },
    {
      bytes: createMammogramBytes({
        laterality: "R",
        viewPosition: "CC",
        rows: 2048,
        columns: 2048,
        sopInstanceUid: "1.2.826.0.1.3680043.10.543.10.2",
      }),
      filename: "large_rcc.dcm",
    },
  ])

  assert.equal(selection.views.rcc?.source, "large_rcc.dcm")
  assert.deepEqual(selection.missingViews.sort(), ["lcc", "lmlo", "rmlo"])
})

test("missing views are reported without throwing", () => {
  const selection = selectPreferredViews([
    { bytes: createMammogramBytes({ laterality: "L", viewPosition: "MLO" }), filename: "lmlo.dcm" },
  ])

  assert.equal(selection.views.lmlo?.source, "lmlo.dcm")
  assert.deepEqual(selection.missingViews.sort(), ["lcc", "rcc", "rmlo"])
})

test("TOMO inputs are excluded from default 2D annotation selection", () => {
  const selection = selectPreferredViews([
    {
      bytes: createMammogramBytes({ mammogramType: "TOMO", laterality: "R", viewPosition: "CC" }),
      filename: "rcc_tomo.dcm",
    },
  ])

  assert.equal(selection.views.rcc, null)
  assert.equal(selection.candidates[0].status, "excluded")
  assert.ok(selection.candidates[0].filterReasons.includes("allowedTypes"))
  assert.ok(selection.candidates[0].filterReasons.includes("allowedDbtObjectKinds"))
})

test("synthetic 2D preference can be selected explicitly", () => {
  const inputs = [
    {
      bytes: createMammogramBytes({
        mammogramType: "FFDM",
        laterality: "R",
        viewPosition: "CC",
        sopInstanceUid: "1.2.826.0.1.3680043.10.543.20.1",
      }),
      filename: "rcc_ffdm.dcm",
    },
    {
      bytes: createMammogramBytes({
        mammogramType: "SYNTH",
        laterality: "R",
        viewPosition: "CC",
        sopInstanceUid: "1.2.826.0.1.3680043.10.543.20.2",
      }),
      filename: "rcc_synth.dcm",
    },
  ]

  assert.equal(selectPreferredViews(inputs).views.rcc?.source, "rcc_ffdm.dcm")
  assert.equal(
    selectPreferredViews(inputs, { preferenceOrder: "synthetic-2d-first" }).views.rcc?.source,
    "rcc_synth.dcm",
  )
})

test("unreadable inputs are returned as structured input errors", () => {
  const selection = selectPreferredViews([
    { bytes: new Uint8Array([1, 2, 3]), filename: "not_dicom.bin" },
  ])

  assert.equal(selection.inputErrors.length, 1)
  assert.equal(selection.inputErrors[0].source, "not_dicom.bin")
  assert.equal(selection.inputErrors[0].code, "dicom_error")
})

test("selectPreferredViewsFromDirectory searches recursively", async (t) => {
  const directory = await tempDir()
  const nested = join(directory, "series")
  await mkdir(nested, { recursive: true })
  await writeMammogramFile(nested, "r_cc.dcm", { laterality: "R", viewPosition: "CC" })

  const selection = selectPreferredViewsFromDirectory(directory)

  assert.equal(selection.views.rcc?.source, join(nested, "r_cc.dcm"))
})

test("directory selection reports unreadable DICOM-like files as input errors", async (t) => {
  const directory = await tempDir()
  await writeFile(join(directory, "not-dicom.dcm"), "not a dicom")

  const selection = selectPreferredViewsFromDirectory(directory)

  assert.equal(selection.inputErrors.length, 1)
  assert.equal(selection.inputErrors[0].code, "dicom_error")
})

function tempDir() {
  return mkdtemp(join(tmpdir(), "mammocat-node-test-"))
}
