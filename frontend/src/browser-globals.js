// Some dependencies still expect the Node-style `global` binding.
if (typeof globalThis.global === "undefined") {
  globalThis.global = globalThis;
}
