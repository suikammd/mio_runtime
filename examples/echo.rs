use futures::StreamExt;
use mio_runtime::executor::Executor;
use mio_runtime::tcp::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn main() {
    let executor = Executor::default();
    executor.block_on(serve);
}

async fn serve() {
    let mut listener = TcpListener::bind("127.0.0.1:11235").unwrap();
    while let Some(ret) = listener.next().await {
        if let Ok(mut stream) = ret {
            println!("accept a new connection successfully");
            let f = async move {
                let mut buf = [0; 4096];
                loop {
                    println!("start read from TcpStream....");
                    match stream.read(&mut buf).await {
                        Ok(n) => {
                            println!("[TcpStream] read size is {} detail is {:?}", n, &buf[..n]);
                            if n == 0 || stream.write_all(&buf[..n]).await.is_err() {
                                return;
                            }
                        }
                        Err(_) => {
                            return;
                        }
                    }
                }
            };
            Executor::spawn(f);
        }
    }
}
