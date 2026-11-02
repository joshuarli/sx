## Tools from an MCP server

A tool imported from an MCP server is called like any tool, with its ID `mcp-SERVER.TOOL`. Leave out
an optional field instead of passing an empty value. When a tool reports an error, the call fails
with kind `Failed` and the server's message, so wrap it in `attempt` when the task expects failures:

```sx
(match (attempt (call mcp-shop.lookup-item {:sku "A-1"}))
  (ok item) (some item)
  (err e) none)
```

An enum value that is not a capitalized name, such as `rush`, is written as a string: `"rush"`.
