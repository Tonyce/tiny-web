# copy tiny-http

1、使用 log，env_loger crate 进行 log 输出

```bash
RUST_LOG=info cargo run --example web
```

log 配置：http://llever.com/rust-cookbook-zh/development_tools/debugging/config_log.zh.html

2、使用 autocannon 进行压测

```bash
autocannon -c 100 -d 5 http://localhost:9876
```

3、lldb 进行 debug
