import {
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

export function createServerOptions(serverPath: string): ServerOptions {
  return {
    command: serverPath,
    transport: TransportKind.stdio,
  };
}
