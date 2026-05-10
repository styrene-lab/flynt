// esbuild's `--loader:.css=text` makes CSS imports resolve to a string
// (the file's contents). TypeScript needs an explicit module declaration
// to accept the import; without this, `import "...css"` fails with
// TS2307 even though the build succeeds.
declare module "*.css" {
  const css: string;
  export default css;
}
