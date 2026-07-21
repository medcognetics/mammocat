import assert from "node:assert/strict"
import { execFile as execFileCallback } from "node:child_process"
import {
  access,
  copyFile,
  mkdir,
  mkdtemp,
  readFile,
  rm,
  writeFile,
} from "node:fs/promises"
import { createRequire } from "node:module"
import { tmpdir } from "node:os"
import { dirname, join, resolve } from "node:path"
import { test } from "node:test"
import { fileURLToPath, pathToFileURL } from "node:url"
import { promisify } from "node:util"

const execFile = promisify(execFileCallback)
const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..")
const packageName = "@medcognetics/mammocat"
const npmCommand = process.platform === "win32" ? "npm.cmd" : "npm"
const maximumCommandOutputBytes = 10 * 1024 * 1024
const minimumNodeVersion = ">=22"
const minimumRustVersion = "1.88"
const nodeCompatiblePromptsVersion = "7.10.1"

test("repository root defines the source-build npm package", async () => {
  const rootPackage = await readJson(join(repositoryRoot, "package.json"))
  const rootPackageLock = await readJson(join(repositoryRoot, "package-lock.json"))
  const nodePackage = await readJson(join(repositoryRoot, "node/package.json"))

  assert.equal(rootPackage.name, packageName)
  assert.equal(rootPackage.version, nodePackage.version)
  assert.equal(rootPackage.private, true)
  assert.equal(rootPackage.main, "node/index.js")
  assert.equal(rootPackage.types, "node/index.d.ts")
  assert.equal(rootPackage.scripts.prepare, "npm run build")
  const napiCliVersion = rootPackage.devDependencies["@napi-rs/cli"]
  assert.match(napiCliVersion, /^\d+\.\d+\.\d+$/)
  assert.equal(napiCliVersion, nodePackage.devDependencies["@napi-rs/cli"])
  assert.equal(
    rootPackageLock.packages["node_modules/@napi-rs/cli"].version,
    napiCliVersion,
  )
  assert.equal(
    rootPackage.overrides["@inquirer/prompts"],
    nodeCompatiblePromptsVersion,
  )
  assert.equal(
    rootPackageLock.packages[
      "node_modules/@napi-rs/cli/node_modules/@inquirer/prompts"
    ].version,
    nodeCompatiblePromptsVersion,
  )
  assert.deepEqual(rootPackage.engines, nodePackage.engines)
  assert.equal(rootPackage.engines.node, minimumNodeVersion)
  assert.equal(rootPackageLock.packages[""].engines.node, minimumNodeVersion)
  assert.equal(rootPackage.optionalDependencies, undefined)
  assert.ok(rootPackage.files.includes("node/*.node"))

  const nodeCargoToml = await readFile(join(repositoryRoot, "node/Cargo.toml"), "utf8")
  assert.match(nodeCargoToml, new RegExp(`^rust-version = "${minimumRustVersion}"$`, "m"))
})

test("commit-pinned Git dependency builds and survives npm ci", async (t) => {
  const nativeFilename = expectedNativeFilename()
  if (nativeFilename === undefined) {
    t.skip(`unsupported test host ${process.platform}-${process.arch}`)
    return
  }

  const temporaryRoot = await mkdtemp(join(tmpdir(), "mammocat-git-install-"))
  t.after(() => rm(temporaryRoot, { force: true, recursive: true }))

  const sourceRepository = join(temporaryRoot, "source")
  const consumer = join(temporaryRoot, "consumer")
  await copyRepositoryFiles(sourceRepository)
  await initializeRepository(sourceRepository)
  const commitSha = (await run("git", ["rev-parse", "HEAD"], sourceRepository)).trim()
  const dependency = `git+${pathToFileURL(sourceRepository).href}#${commitSha}`

  await run(
    npmCommand,
    ["ci", "--ignore-scripts", "--omit=optional", "--no-audit", "--no-fund"],
    sourceRepository,
  )
  await rm(join(sourceRepository, "node_modules"), { force: true, recursive: true })

  await mkdir(consumer)
  await writeFile(
    join(consumer, "package.json"),
    `${JSON.stringify(
      {
        name: "mammocat-git-consumer",
        private: true,
        version: "1.0.0",
        dependencies: { [packageName]: dependency },
      },
      null,
      2,
    )}\n`,
  )

  await run(npmCommand, ["install", "--omit=optional", "--no-audit", "--no-fund"], consumer)
  await assertInstalledPackage(consumer, dependency, commitSha, nativeFilename)

  await rm(join(consumer, "node_modules"), { force: true, recursive: true })
  await run(npmCommand, ["ci", "--omit=optional", "--no-audit", "--no-fund"], consumer)
  await assertInstalledPackage(consumer, dependency, commitSha, nativeFilename)
})

async function assertInstalledPackage(
  consumer,
  dependency,
  commitSha,
  nativeFilename,
) {
  const requireFromConsumer = createRequire(join(consumer, "package.json"))
  const mammocat = requireFromConsumer(packageName)

  assert.equal(typeof mammocat.extractMetadata, "function")
  assert.equal(typeof mammocat.selectPreferredViews, "function")

  const installedPackage = dirname(requireFromConsumer.resolve(`${packageName}/package.json`))
  assert.equal(requireFromConsumer.resolve(packageName), join(installedPackage, "node", "index.js"))
  await access(join(installedPackage, "node", nativeFilename))

  const lockfile = await readJson(join(consumer, "package-lock.json"))
  assert.equal(lockfile.packages[""].dependencies[packageName], dependency)
  assert.ok(
    lockfile.packages[`node_modules/${packageName}`].resolved.endsWith(`#${commitSha}`),
    "package-lock.json must retain the full commit SHA",
  )
}

async function copyRepositoryFiles(destinationRoot) {
  const { stdout } = await execFile(
    "git",
    ["ls-files", "--cached", "--others", "--exclude-standard", "-z"],
    { cwd: repositoryRoot, encoding: "buffer", maxBuffer: maximumCommandOutputBytes },
  )

  const paths = stdout.toString("utf8").split("\0").filter(Boolean)
  for (const relativePath of paths) {
    const destination = join(destinationRoot, relativePath)
    await mkdir(dirname(destination), { recursive: true })
    await copyFile(join(repositoryRoot, relativePath), destination)
  }
}

async function initializeRepository(repository) {
  await run("git", ["init", "--quiet"], repository)
  await run("git", ["config", "user.email", "test@example.invalid"], repository)
  await run("git", ["config", "user.name", "Mammocat Test"], repository)
  await run("git", ["add", "."], repository)
  await run("git", ["commit", "--quiet", "-m", "Test source package"], repository)
}

function expectedNativeFilename() {
  if (process.platform === "linux" && process.arch === "x64") {
    const glibcVersion = process.report?.getReport().header.glibcVersionRuntime
    return glibcVersion === undefined ? undefined : "mammocat.linux-x64-gnu.node"
  }
  if (process.platform === "darwin" && process.arch === "x64") {
    return "mammocat.darwin-x64.node"
  }
  if (process.platform === "darwin" && process.arch === "arm64") {
    return "mammocat.darwin-arm64.node"
  }
  if (process.platform === "win32" && process.arch === "x64") {
    return "mammocat.win32-x64-msvc.node"
  }
  return undefined
}

async function readJson(path) {
  return JSON.parse(await readFile(path, "utf8"))
}

async function run(command, args, cwd) {
  const { stdout } = await execFile(command, args, {
    cwd,
    env: {
      ...process.env,
      npm_config_audit: "false",
      npm_config_engine_strict: "true",
      npm_config_fund: "false",
    },
    maxBuffer: maximumCommandOutputBytes,
  })
  return stdout
}
