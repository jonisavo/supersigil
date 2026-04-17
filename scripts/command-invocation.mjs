export function buildCommandInvocation(command, args) {
  if (process.platform === "win32") {
    if (command === "pnpm") {
      return {
        command: "cmd.exe",
        args: ["/d", "/s", "/c", "pnpm", ...args],
      };
    }

    if (/\.(?:[cm]?js)$/i.test(command)) {
      return {
        command: process.execPath,
        args: [command, ...args],
      };
    }
  }

  return { command, args };
}
