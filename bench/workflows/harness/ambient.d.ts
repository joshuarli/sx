interface BunSink {
  write(data: string): number;
  flush(): number | Promise<number>;
  end(): number | Promise<number>;
}

interface BunSubprocess {
  readonly stdin: BunSink;
  readonly stdout: ReadableStream<Uint8Array>;
  readonly exited: Promise<number>;
  kill(signal?: number): void;
}

declare const Bun: {
  spawn(command: string[], options: { stdin: "pipe"; stdout: "pipe"; stderr: "inherit" }): BunSubprocess;
};

declare const process: {
  argv: string[];
  stdout: { write(text: string): boolean };
  exit(code: number): never;
};
