use std::fs;

use anyhow::{Result, anyhow};
use hyper_util::{
    rt::{TokioExecutor, TokioIo},
    server::conn::auto::Builder as ConnectionBuilder,
};
use object_storage_perf::{
    benchmark::{MIB, MULTIPART_PART_SIZE, READ_SIZE},
    config::StorageConfig,
    storage::Storage,
};
use s3s::{auth::SimpleAuth, service::S3ServiceBuilder};
use s3s_fs::FileSystem;
use tempfile::TempDir;
use tokio::{
    net::TcpListener,
    sync::oneshot,
    task::{JoinHandle, JoinSet},
};

const BUCKET: &str = "benchmark";
const ACCESS_KEY: &str = "access-key";
const SECRET_KEY: &str = "secret-key";

struct LocalS3 {
    endpoint: String,
    stop: Option<oneshot::Sender<()>>,
    server: Option<JoinHandle<()>>,
    _root: TempDir,
}

impl LocalS3 {
    async fn start() -> Result<Self> {
        let root = TempDir::new()?;
        fs::create_dir(root.path().join(BUCKET))?;

        let filesystem = FileSystem::new(root.path()).map_err(|error| anyhow!("{error:?}"))?;
        let mut service = S3ServiceBuilder::new(filesystem);
        service.set_auth(SimpleAuth::from_single(ACCESS_KEY, SECRET_KEY));
        let service = service.build();

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let (stop, mut stop_requested) = oneshot::channel();
        let server = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    _ = &mut stop_requested => break,
                    accepted = listener.accept() => {
                        let Ok((socket, _)) = accepted else {
                            break;
                        };
                        let service = service.clone();
                        connections.spawn(async move {
                            let builder = ConnectionBuilder::new(TokioExecutor::new());
                            let connection =
                                builder.serve_connection(TokioIo::new(socket), service);
                            let _ = connection.await;
                        });
                    }
                }
            }

            connections.abort_all();
            while connections.join_next().await.is_some() {}
        });

        Ok(Self {
            endpoint: format!("http://{address}"),
            stop: Some(stop),
            server: Some(server),
            _root: root,
        })
    }

    async fn shutdown(mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(server) = self.server.take() {
            let _ = server.await;
        }
    }
}

impl Drop for LocalS3 {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(server) = self.server.take() {
            server.abort();
        }
    }
}

#[tokio::test]
async fn opendal_performs_multipart_write_range_read_and_stat() -> Result<()> {
    let server = LocalS3::start().await?;
    let storage = Storage::new(StorageConfig {
        endpoint: server.endpoint.clone(),
        bucket: BUCKET.to_owned(),
        region: "us-east-1".to_owned(),
        access_key_id: ACCESS_KEY.to_owned(),
        secret_access_key: SECRET_KEY.to_owned(),
        prefix: "test".to_owned(),
    })?;
    let operator = storage.operator();
    let path = "test/object";

    let mut writer = operator
        .writer_with(path)
        .chunk(MULTIPART_PART_SIZE as usize)
        .concurrent(2)
        .await?;
    writer
        .write(vec![0x5a; MULTIPART_PART_SIZE as usize])
        .await?;
    writer.write(vec![0xa5; (2 * MIB) as usize]).await?;
    writer.close().await?;

    let metadata = operator.stat(path).await?;
    assert_eq!(metadata.content_length(), 12 * MIB);

    let bytes = operator.read_with(path).range(0..READ_SIZE).await?;
    assert_eq!(bytes.len() as u64, READ_SIZE);
    assert!(bytes.to_vec().iter().all(|byte| *byte == 0x5a));

    operator.delete(path).await?;
    server.shutdown().await;
    Ok(())
}
