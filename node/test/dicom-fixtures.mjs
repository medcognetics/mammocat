import { mkdir, writeFile } from "node:fs/promises"
import { join } from "node:path"

const DIGITAL_MAMMOGRAPHY_SOP_CLASS_UID = "1.2.840.10008.5.1.4.1.1.1.2"
const BREAST_TOMOSYNTHESIS_SOP_CLASS_UID = "1.2.840.10008.5.1.4.1.1.13.1.3"
const EXPLICIT_VR_LITTLE_ENDIAN = "1.2.840.10008.1.2.1"
const IMPLEMENTATION_CLASS_UID = "1.2.826.0.1.3680043.10.543.74"
const VIEW_CODES = {
  CC: ["399162004", "cranio-caudal"],
  MLO: ["399368009", "medio-lateral oblique"],
}
const VIEW_MODIFIER_CODES = {
  "implant displaced": ["399209000", "Implant Displaced"],
  magnification: ["399163009", "Magnification"],
  "spot compression": ["399055006", "Spot Compression"],
}

export async function writeMammogramFile(directory, fileName, options = {}) {
  await mkdir(directory, { recursive: true })
  const path = join(directory, fileName)
  await writeFile(path, createMammogramBytes(options))
  return path
}

export function createMammogramBytes(options = {}) {
  const {
    mammogramType = "FFDM",
    laterality = "L",
    viewPosition = "MLO",
    rows = 2048,
    columns = 1536,
    studyInstanceUid = "1.2.826.0.1.3680043.10.543.1",
    seriesInstanceUid = `${studyInstanceUid}.1`,
    sopInstanceUid = `${seriesInstanceUid}.1`,
    pixelSpacing = ["0.07", "0.07"],
    nestedViewModifiers = [],
  } = options

  const sopClassUid =
    mammogramType === "TOMO"
      ? BREAST_TOMOSYNTHESIS_SOP_CLASS_UID
      : DIGITAL_MAMMOGRAPHY_SOP_CLASS_UID
  const imageType = imageTypeForMammogramType(mammogramType)
  const numberOfFrames = mammogramType === "TOMO" ? "50" : "1"

  const metaElements = [
    element(0x0002, 0x0002, "UI", sopClassUid),
    element(0x0002, 0x0003, "UI", sopInstanceUid),
    element(0x0002, 0x0010, "UI", EXPLICIT_VR_LITTLE_ENDIAN),
    element(0x0002, 0x0012, "UI", IMPLEMENTATION_CLASS_UID),
  ]
  const metaBody = concat(metaElements)
  const preamble = new Uint8Array(132)
  preamble.set([0x44, 0x49, 0x43, 0x4d], 128)

  const dataset = concat([
    element(0x0008, 0x0008, "CS", imageType),
    element(0x0008, 0x0016, "UI", sopClassUid),
    element(0x0008, 0x0018, "UI", sopInstanceUid),
    element(0x0008, 0x0060, "CS", "MG"),
    element(0x0008, 0x103e, "LO", seriesDescriptionForMammogramType(mammogramType)),
    element(0x0018, 0x5101, "CS", viewPosition),
    element(0x0020, 0x000d, "UI", studyInstanceUid),
    element(0x0020, 0x000e, "UI", seriesInstanceUid),
    element(0x0020, 0x0062, "CS", laterality),
    element(0x0028, 0x0008, "IS", numberOfFrames),
    element(0x0028, 0x0010, "US", rows),
    element(0x0028, 0x0011, "US", columns),
    element(0x0028, 0x0030, "DS", pixelSpacing.join("\\")),
    element(0x0008, 0x0068, "CS", "FOR PRESENTATION"),
    element(0x0028, 0x2110, "CS", "00"),
    nestedViewModifiers.length > 0
      ? viewCodeSequence(viewPosition, nestedViewModifiers)
      : new Uint8Array(),
  ])

  return Buffer.from(
    concat([preamble, groupLengthElement(metaBody.length), metaBody, dataset]),
  )
}

function viewCodeSequence(viewPosition, modifierMeanings) {
  const modifierItems = modifierMeanings.map((meaning) => {
    const [codeValue, codeMeaning] = VIEW_MODIFIER_CODES[meaning.toLowerCase()]
    return concat([
      element(0x0008, 0x0100, "SH", codeValue),
      element(0x0008, 0x0102, "SH", "SCT"),
      element(0x0008, 0x0104, "LO", codeMeaning),
    ])
  })
  const [viewCodeValue, viewCodeMeaning] = VIEW_CODES[viewPosition]
  const viewItem = concat([
    element(0x0008, 0x0100, "SH", viewCodeValue),
    element(0x0008, 0x0102, "SH", "SCT"),
    element(0x0008, 0x0104, "LO", viewCodeMeaning),
    sequenceElement(0x0054, 0x0222, modifierItems),
  ])
  return sequenceElement(0x0054, 0x0220, [viewItem])
}

function sequenceElement(group, tag, itemDatasets) {
  const value = concat(itemDatasets.map(sequenceItem))
  const header = new Uint8Array(12)
  const view = new DataView(header.buffer)
  view.setUint16(0, group, true)
  view.setUint16(2, tag, true)
  header[4] = 0x53
  header[5] = 0x51
  view.setUint32(8, value.length, true)
  return concat([header, value])
}

function sequenceItem(dataset) {
  const header = new Uint8Array(8)
  const view = new DataView(header.buffer)
  view.setUint16(0, 0xfffe, true)
  view.setUint16(2, 0xe000, true)
  view.setUint32(4, dataset.length, true)
  return concat([header, dataset])
}

function imageTypeForMammogramType(mammogramType) {
  switch (mammogramType) {
    case "SYNTH":
      return "DERIVED\\PRIMARY\\TOMO_2D"
    case "TOMO":
      return "ORIGINAL\\PRIMARY\\VOLUME"
    default:
      return "ORIGINAL\\PRIMARY"
  }
}

function seriesDescriptionForMammogramType(mammogramType) {
  return mammogramType === "SYNTH" ? "Synthetic 2D s-view" : "Mammography"
}

function groupLengthElement(length) {
  const bytes = new Uint8Array(12)
  const view = new DataView(bytes.buffer)
  view.setUint16(0, 0x0002, true)
  view.setUint16(2, 0x0000, true)
  bytes[4] = 0x55
  bytes[5] = 0x4c
  view.setUint16(6, 4, true)
  view.setUint32(8, length, true)
  return bytes
}

function element(group, tag, vr, value) {
  const valueBytes = valueToBytes(vr, value)
  const header = new Uint8Array(8)
  const view = new DataView(header.buffer)
  view.setUint16(0, group, true)
  view.setUint16(2, tag, true)
  header[4] = vr.charCodeAt(0)
  header[5] = vr.charCodeAt(1)
  view.setUint16(6, valueBytes.length, true)
  return concat([header, valueBytes])
}

function valueToBytes(vr, value) {
  if (vr === "US") {
    const bytes = new Uint8Array(2)
    new DataView(bytes.buffer).setUint16(0, value, true)
    return bytes
  }

  const encoded = Buffer.from(String(value), "ascii")
  const needsNullPadding = vr === "UI"
  const paddingLength = encoded.length % 2 === 0 ? 0 : 1
  if (paddingLength === 0) {
    return encoded
  }

  const padded = new Uint8Array(encoded.length + 1)
  padded.set(encoded)
  padded[padded.length - 1] = needsNullPadding ? 0 : 0x20
  return padded
}

function concat(chunks) {
  const length = chunks.reduce((total, chunk) => total + chunk.length, 0)
  const output = new Uint8Array(length)
  let offset = 0
  for (const chunk of chunks) {
    output.set(chunk, offset)
    offset += chunk.length
  }
  return output
}
