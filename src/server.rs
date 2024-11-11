use tonic::{transport::Server};
use tonic::{Request, Response, Status};

use xproto::boxer::v1::boxer_service_server::{BoxerService, BoxerServiceServer};
use xproto::boxer::v1::{GetRequest, GetResponse, PostRequest, PostResponse, BeaconRequest, BeaconResponse, TheResponse};
use xproto::google::protobuf::Empty;

#[derive(Debug, Default)]
pub struct Boxer {}

#[tonic::async_trait]
impl BoxerService for Boxer {
    async fn beacon(&self, request: Request<BeaconRequest>, ) -> Result<Response<BeaconResponse>, Status> {
        println!("Got a request: {:?}", request);
        let reply = BeaconResponse { res: Some(Empty{}) };
        Ok(Response::new(reply))
    }
    async fn get(&self, request: Request<GetRequest>, ) -> Result<Response<GetResponse>, Status> {
        println!("Got a request: {:?}", request);
        let reply = GetResponse {
            res: Some(TheResponse {
                status: format!("200"),
                body: "".to_string(),
                idempotency_key: request.into_inner().req.unwrap().idempotency_key
            })
        };
        Ok(Response::new(reply))
    }
    async fn post(&self, request: Request<PostRequest>, ) -> Result<Response<PostResponse>, Status> {
        println!("Got a request: {:?}", request);
        let reply = PostResponse {
            res: Some(TheResponse {
                status: format!("200"),
                body: "".to_string(),
                idempotency_key: request.into_inner().req.unwrap().idempotency_key
            })
        };
        Ok(Response::new(reply))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "[::1]:50051".parse()?;
    let boxer = Boxer::default();

    Server::builder()
        .add_service(BoxerServiceServer::new(boxer))
        .serve(addr)
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn dummy_test() {
        assert_eq!(1, 1);
    }
}
