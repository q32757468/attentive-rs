# attentive-notifier

`attentive-notifier` 是 `@attentive-kit/notifier` 的 Rust 实现，提供相同的 HTTP 通知协议、可替换的通知 dispatcher，以及 Windows Toast 默认实现。也包含 `attentive-notifier` 命令行程序。

## HTTP 协议

- `GET /health` 返回 `{"status":"ok"}`。
- `POST /api/v1/notifications` 接收 `title`、`body`，以及可选的 `source`、`action` 和 `metadata`。
- `action` 目前只支持 `{"type":"open-uri","uri":"..."}`；URI scheme 支持 `http`、`https` 和 `vscode`。
- 请求体默认上限为 1 MiB。无效请求、过大请求和 dispatcher 错误分别返回结构化的 `400`、`413` 和 `500` 响应。
- 成功提交返回 `201` 和唯一的 `notificationId`。

## 运行服务

```sh
cargo run --bin attentive-notifier -- --host 0.0.0.0 --port 8769
```

也可以通过 `ATTENTIVE_NOTIFIER_HOST` 和 `ATTENTIVE_NOTIFIER_PORT` 设置监听地址。默认 dispatcher 使用 Windows Toast；其他平台默认返回错误，可通过 Rust API 注入自己的 dispatcher。

## 作为库使用

```rust,no_run
use attentive_notifier::{
    start_notifier, NotifierServerOptions, StartNotifierOptions,
};

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let server_options = NotifierServerOptions::default()
        .with_sync_dispatcher(|request, notification_id| {
            println!("{} ({notification_id})", request.title);
            Ok(())
        });

    let server = start_notifier(
        StartNotifierOptions::default()
            .with_host("127.0.0.1")
            .with_port(8769)
            .with_server_options(server_options),
    )
    .await?;

    println!("Listening on {}", server.local_addr());
    tokio::signal::ctrl_c().await?;
    server.shutdown().await
}
```

`create_notifier_router` 也可以嵌入现有的 Axum 应用。项目采用 MIT 许可证，详见仓库根目录的 `LICENSE` 文件。
