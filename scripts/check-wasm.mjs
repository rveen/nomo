// Determinism gates on the built WebAssembly artifact.
//
// These read the module itself rather than the build that produced it, so they
// hold even if someone changes a flag or a profile.
//
//   1. The import section must be EMPTY.
//   2. No SIMD feature may appear in `target_features`.
//
// Gate 1 is the strong one. The design's central risk is a transcendental coming
// from the host instead of the libm compiled into the artifact (design note §3),
// and a module that imports nothing cannot call the host at all — not
// `Math.sin`, not a platform libm, not anything. It is a stronger statement than
// "imports no math", and it is exact.
//
// Gate 2 covers relaxed SIMD, whose `relaxed_madd` is explicitly nondeterministic
// — a single rounding on hardware with FMA and a double rounding without — which
// is precisely the drift this design exists to prevent.

import { compile, imports, targetFeatures, wasmPath, repoRoot } from "./wasm-host.mjs";
import path from "node:path";

const file = process.argv[2] ?? wasmPath;
const failures = [];

const { bytes, module } = await compile(file);

const declared = imports(module);
if (declared.length > 0) {
  failures.push(
    `the module declares ${declared.length} import(s); it must declare none:\n` +
      declared.map((i) => `    ${i.module}.${i.name} (${i.kind})`).join("\n") +
      "\n    -> an import is a call into the host, and the host's maths is not ours",
  );
}

const features = targetFeatures(bytes);
if (features === null) {
  failures.push(
    "no `target_features` section, so the SIMD gate cannot be checked\n" +
      "    -> a stripped artifact cannot be verified; check with an unstripped build",
  );
} else {
  const simd = features.filter((f) => f.name.includes("simd"));
  if (simd.length > 0) {
    failures.push(
      `SIMD features are enabled: ${simd.map((f) => f.prefix + f.name).join(", ")}\n` +
        "    -> relaxed SIMD is nondeterministic by specification; see design note §3",
    );
  }
}

const relative = path.relative(repoRoot, file) || file;
if (failures.length > 0) {
  console.error(`check-wasm: ${relative}`);
  for (const failure of failures) console.error(`  error: ${failure}`);
  process.exit(1);
}

const names = features.map((f) => f.prefix + f.name).join(" ");
console.log(`ok: ${relative} imports nothing; features: ${names}`);
