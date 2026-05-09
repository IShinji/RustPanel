# Backend Pattern Selection Standard

- Service / Read Model 是业务逻辑唯一来源。
- Transport adapter 只做参数提取、鉴权上下文传递和响应映射。
- 数据库访问放在 services/repositories，不放在 HTTP/gRPC handler。
- 复杂聚合、权限、写操作默认 Service-first。
