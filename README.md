# Sickle
A Rust multi-thread asyn io lib

## 项目特点
- 不使用await async的纯异步实现
- 使用Rust闭包封装成任务 并基于回调的方式处理任务 代码清晰
- 底层事件系统使用epoll ET多线程模式处理网络IO
- 底层事件系统线程安全
- 仅支持linux平台
- 了解更多: [mio](https://github.com/tokio-rs/mio)

## 特性
- 网络库
  - tcp客户端/服务端/tunnel
  - 对套接字多种操作的封装
- 其它
  - TODO

## 编译
-  cargo [+nightly] build [--example [example_name]]

## QA 该库性能怎么样
- 与nginx实测对比 是其性能的70% [测试](https://github.com/hankai17/context_benchmark/tree/master/rust)

## 联系方式
- 邮箱：<hankai17@126.com>
