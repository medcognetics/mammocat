import assert from "node:assert/strict"
import { mkdir, mkdtemp, readFile, writeFile } from "node:fs/promises"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { test } from "node:test"

import {
  assessConventionalOrientation,
  extractMetadata,
  selectPreferredViews,
  selectPreferredViewsFromDirectory,
} from "../index.js"
import { createMammogramBytes, writeMammogramFile } from "./dicom-fixtures.mjs"

const minimumNodeVersion = ">=22"

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
  assert.deepEqual(metadata.viewModifiers, [])
  assert.deepEqual(metadata.conventionalOrientation, {
    status: "matches",
    expectedComponents: ["A", "FR"],
    observedComponents: ["A", "FR"],
    horizontalFlipRequired: false,
    verticalFlipRequired: false,
  })
  assert.deepEqual(metadata.pixelSpacing, { row: 0.07, column: 0.07 })
  assert.doesNotThrow(() => JSON.stringify(metadata))
})

test("assessConventionalOrientation accepts arbitrary DICOM modality and SOP class", () => {
  const assessment = assessConventionalOrientation({
    bytes: createMammogramBytes({
      laterality: "R",
      viewPosition: "MLO",
      patientOrientation: ["A", "HR"],
      modality: "CT",
      sopClassUid: "1.2.840.10008.5.1.4.1.1.2",
    }),
    filename: "ungated-orientation.dcm",
  })

  assert.deepEqual(assessment, {
    status: "requires_flip",
    expectedComponents: ["P", "FL"],
    observedComponents: ["A", "HR"],
    horizontalFlipRequired: true,
    verticalFlipRequired: true,
  })
})

test("synthesized metadata uses the canonical machine value", () => {
  const metadata = extractMetadata({
    bytes: createMammogramBytes({ mammogramType: "SYNTH" }),
    filename: "synth.dcm",
  })

  assert.equal(metadata.mammogramType, "synth")
})

test("DBT MIP metadata uses the canonical machine value", () => {
  const metadata = extractMetadata({
    bytes: createMammogramBytes({ mammogramType: "DBT_MIP" }),
    filename: "dbt_mip.dcm",
  })

  assert.equal(metadata.mammogramType, "dbt-mip")
  assert.equal(metadata.dbtObjectKind, "none")
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

test("nested view modifiers populate all metadata flags", () => {
  const metadata = extractMetadata({
    bytes: createMammogramBytes({
      nestedViewModifiers: ["implant displaced", "spot compression", "magnification"],
    }),
    filename: "nested_modifiers.dcm",
  })

  assert.equal(metadata.isImplantDisplaced, true)
  assert.equal(metadata.isSpotCompression, true)
  assert.equal(metadata.isMagnified, true)
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

test("colliding filenames and SOP UIDs retain the selected input identity", () => {
  const sharedOptions = {
    laterality: "R",
    viewPosition: "CC",
    studyInstanceUid: "1.2.826.0.1.3680043.10.543.39",
    seriesInstanceUid: "1.2.826.0.1.3680043.10.543.39.1",
    sopInstanceUid: "1.2.826.0.1.3680043.10.543.39.1.1",
  }
  const selection = selectPreferredViews([
    {
      bytes: createMammogramBytes({ ...sharedOptions, rows: 1024, columns: 1024 }),
      filename: "duplicate.dcm",
    },
    {
      bytes: createMammogramBytes({ ...sharedOptions, rows: 2048, columns: 2048 }),
      filename: "duplicate.dcm",
    },
  ])

  assert.equal(selection.views.rcc?.source, "duplicate.dcm")
  assert.equal(selection.views.rcc?.inputIndex, 1)
  assert.equal(selection.candidates[0].status, "unused")
  assert.deepEqual(selection.candidates[0].selectedAs, [])
  assert.equal(selection.candidates[1].status, "selected")
  assert.deepEqual(selection.candidates[1].selectedAs, ["rcc"])
})

test("missing views are reported without throwing", () => {
  const selection = selectPreferredViews([
    { bytes: createMammogramBytes({ laterality: "L", viewPosition: "MLO" }), filename: "lmlo.dcm" },
  ])

  assert.equal(selection.views.lmlo?.source, "lmlo.dcm")
  assert.deepEqual(selection.missingViews.sort(), ["lcc", "rcc", "rmlo"])
})

test("fallback view policy controls non-standard slot candidates", () => {
  const inputs = [
    { bytes: createMammogramBytes({ laterality: "L", viewPosition: "ML" }), filename: "lml.dcm" },
  ]

  const standardOnly = selectPreferredViews(inputs, {
    viewFallbackPolicy: { mode: "standard-only" },
  })
  const defaultPolicy = selectPreferredViews(inputs)
  const mlAllowed = selectPreferredViews(inputs, {
    viewFallbackPolicy: { mode: "allow-list", allowedViews: ["ml"] },
  })

  assert.equal(standardOnly.views.lmlo, null)
  assert.ok(standardOnly.candidates[0].filterReasons.includes("viewFallbackPolicy"))
  assert.equal(defaultPolicy.views.lmlo?.source, "lml.dcm")
  assert.equal(mlAllowed.views.lmlo?.source, "lml.dcm")
})

test("modifier policy requires every modifier on compound views", () => {
  const inputs = [
    {
      bytes: createMammogramBytes({
        laterality: "L",
        viewPosition: "MLO",
        nestedViewModifiers: ["implant displaced", "spot compression"],
      }),
      filename: "compound.dcm",
    },
  ]

  const partiallyAllowed = selectPreferredViews(inputs, {
    viewModifierPolicy: { mode: "allow-list", allowedModifiers: ["implant-displaced"] },
  })
  const fullyAllowed = selectPreferredViews(inputs, {
    viewModifierPolicy: {
      mode: "allow-list",
      allowedModifiers: ["implant-displaced", "spot-compression"],
    },
  })

  assert.equal(partiallyAllowed.views.lmlo, null)
  assert.ok(partiallyAllowed.candidates[0].filterReasons.includes("viewModifierPolicy"))
  assert.equal(fullyAllowed.views.lmlo?.source, "compound.dcm")
})

test("invalid view policy objects throw actionable argument errors", () => {
  const inputs = [
    { bytes: createMammogramBytes({ laterality: "L", viewPosition: "ML" }), filename: "lml.dcm" },
  ]

  assert.throws(
    () =>
      selectPreferredViews(inputs, {
        viewFallbackPolicy: { mode: "allow-list" },
      }),
    /allowedViews is required/,
  )
  assert.throws(
    () =>
      selectPreferredViews(inputs, {
        viewFallbackPolicy: { mode: "allow-list", allowedViews: ["fb"] },
      }),
    /Unsupported fallback view/,
  )
  assert.throws(
    () =>
      selectPreferredViews(inputs, {
        viewModifierPolicy: { mode: "unmodified-only", allowedModifiers: ["magnification"] },
      }),
    /allowedModifiers must be omitted/,
  )
})

test("selection failures report missing view slot keys", () => {
  const selection = selectPreferredViews(
    [
      {
        bytes: createMammogramBytes({
          laterality: "R",
          viewPosition: "CC",
          studyInstanceUid: "1.2.826.0.1.3680043.10.543.30.1",
        }),
        filename: "study_a_rcc.dcm",
      },
      {
        bytes: createMammogramBytes({
          laterality: "L",
          viewPosition: "MLO",
          studyInstanceUid: "1.2.826.0.1.3680043.10.543.30.2",
        }),
        filename: "study_b_lmlo.dcm",
      },
    ],
    { strict: true },
  )

  assert.deepEqual(selection.missingViews.sort(), ["lcc", "lmlo", "rcc", "rmlo"])
  assert.ok(selection.warnings.some((warning) => warning.startsWith("Selection failed:")))
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

test("DBT MIPs remain eligible for default 2D annotation selection", () => {
  const selection = selectPreferredViews([
    {
      bytes: createMammogramBytes({
        mammogramType: "DBT_MIP",
        laterality: "R",
        viewPosition: "CC",
      }),
      filename: "rcc_dbt_mip.dcm",
    },
  ])

  assert.equal(selection.views.rcc?.source, "rcc_dbt_mip.dcm")
  assert.equal(selection.views.rcc?.metadata.mammogramType, "dbt-mip")
  assert.equal(selection.candidates[0].status, "selected")
  assert.deepEqual(selection.candidates[0].filterReasons, [])
})

test("tomo-first can select TOMO inputs explicitly", () => {
  const selection = selectPreferredViews(
    [
      {
        bytes: createMammogramBytes({
          mammogramType: "TOMO",
          laterality: "R",
          viewPosition: "CC",
        }),
        filename: "rcc_tomo.dcm",
      },
    ],
    { preferenceOrder: "tomo-first" },
  )

  assert.equal(selection.views.rcc?.source, "rcc_tomo.dcm")
  assert.equal(selection.candidates[0].status, "selected")
  assert.deepEqual(selection.candidates[0].filterReasons, [])
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

test("package metadata wires supported native packages", async () => {
  const packageJson = JSON.parse(
    await readFile(new URL("../package.json", import.meta.url), "utf8"),
  )
  const packageLock = JSON.parse(
    await readFile(new URL("../package-lock.json", import.meta.url), "utf8"),
  )
  const supportedPackages = {
    "@medcognetics/mammocat-darwin-arm64": {
      cpu: ["arm64"],
      directory: "darwin-arm64",
      main: "mammocat.darwin-arm64.node",
      os: ["darwin"],
      target: "aarch64-apple-darwin",
    },
    "@medcognetics/mammocat-darwin-x64": {
      cpu: ["x64"],
      directory: "darwin-x64",
      main: "mammocat.darwin-x64.node",
      os: ["darwin"],
      target: "x86_64-apple-darwin",
    },
    "@medcognetics/mammocat-linux-x64-gnu": {
      cpu: ["x64"],
      directory: "linux-x64-gnu",
      libc: ["glibc"],
      main: "mammocat.linux-x64-gnu.node",
      os: ["linux"],
      target: "x86_64-unknown-linux-gnu",
    },
    "@medcognetics/mammocat-win32-x64-msvc": {
      cpu: ["x64"],
      directory: "win32-x64-msvc",
      main: "mammocat.win32-x64-msvc.node",
      os: ["win32"],
      target: "x86_64-pc-windows-msvc",
    },
  }

  assert.deepEqual(packageJson.files, [
    "index.js",
    "index.d.ts",
    "LICENSE.md",
    "README.md",
    "package.json",
  ])
  assert.equal(packageJson.engines.node, minimumNodeVersion)
  assert.equal(packageLock.packages[""].engines.node, minimumNodeVersion)
  assert.deepEqual(
    Object.keys(packageJson.optionalDependencies).sort(),
    Object.keys(supportedPackages).sort(),
  )
  assert.deepEqual(
    [...packageJson.napi.targets].sort(),
    Object.values(supportedPackages)
      .map(({ target }) => target)
      .sort(),
  )

  for (const [packageName, expected] of Object.entries(supportedPackages)) {
    assert.equal(packageJson.optionalDependencies[packageName], packageJson.version)

    const nativePackageJson = JSON.parse(
      await readFile(
        new URL(`../npm/${expected.directory}/package.json`, import.meta.url),
        "utf8",
      ),
    )

    assert.equal(nativePackageJson.name, packageName)
    assert.equal(nativePackageJson.version, packageJson.version)
    assert.equal(nativePackageJson.main, expected.main)
    assert.deepEqual(nativePackageJson.files, [expected.main])
    assert.deepEqual(nativePackageJson.os, expected.os)
    assert.deepEqual(nativePackageJson.cpu, expected.cpu)
    assert.deepEqual(nativePackageJson.libc, expected.libc)
    assert.equal(nativePackageJson.description, packageJson.description)
    assert.equal(nativePackageJson.license, packageJson.license)
    assert.deepEqual(nativePackageJson.engines, packageJson.engines)
    assert.equal(
      packageLock.packages[`node_modules/${packageName}`].engines.node,
      minimumNodeVersion,
    )
  }
})

function tempDir() {
  return mkdtemp(join(tmpdir(), "mammocat-node-test-"))
}
