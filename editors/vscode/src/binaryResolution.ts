import { posix, win32 } from "path";

export function serverBinaryName(platform: NodeJS.Platform): string {
  return platform === "win32" ? "supersigil-lsp.exe" : "supersigil-lsp";
}

export function pathLookupCommand(platform: NodeJS.Platform): string {
  return platform === "win32"
    ? `where.exe ${serverBinaryName(platform)}`
    : `which ${serverBinaryName(platform)}`;
}

export function defaultServerBinaryCandidates(
  homeDir: string,
  platform: NodeJS.Platform,
): string[] {
  const binaryName = serverBinaryName(platform);
  const pathModule = platform === "win32" ? win32 : posix;

  if (platform === "win32") {
    return [pathModule.join(homeDir, ".cargo", "bin", binaryName)];
  }

  return [
    pathModule.join(homeDir, ".cargo", "bin", binaryName),
    pathModule.join(homeDir, ".local", "bin", binaryName),
  ];
}
