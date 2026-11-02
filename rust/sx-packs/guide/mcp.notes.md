### How the MCP server's tools behave

- A tool that reports an error fails the call with kind `Failed` and the server's message.
- An optional input left out is not sent.
- A tool without a declared result returns its text.
