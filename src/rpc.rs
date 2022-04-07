//! ChiselStore RPC module.

//use crate::rpc::proto::rpc_server::Rpc;
use crate::rpc::proto::*; 
use crate::{ StoreCommand, StoreServer, StoreTransport};
use async_mutex::Mutex;
use async_trait::async_trait;
use crate::rpc::proto::rpc_server::Rpc;
use crossbeam::queue::ArrayQueue;
use derivative::Derivative;
//use little_raft::message::Message;
use omnipaxos_core::messages::Message;


use omnipaxos_core::messages::{
    AcceptDecide, AcceptStopSign, AcceptSync, Accepted, 
    AcceptedStopSign, Decide, DecideStopSign, FirstAccept, 
     Prepare, Promise, Compaction, PaxosMsg
   
};

use omnipaxos_core::ballot_leader_election::messages::{
    BLEMessage, HeartbeatMsg, 
    HeartbeatRequest, HeartbeatReply
};



use omnipaxos_core::util::SyncItem;



use omnipaxos_core::storage::{SnapshotType, StopSign};

use omnipaxos_core::ballot_leader_election::Ballot;


use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;
use tonic::{Request, Response, Status};

#[allow(missing_docs)]
pub mod proto {
    tonic::include_proto!("proto");
}



use proto::rpc_client::RpcClient;
/* use proto::{
    Query, QueryResults, QueryRow, Void ,Ballot,Entry,Entries,SyncItem,None,StopSign,Metadata,
    Prepare,Promise,AcceptSync,FirstAccept,AcceptDecide,Accepted,Decide,AcceptStopSign,ProposalForward,
    AcceptedStopSign,DecideStopSign,Compaction,PaxosMsg,BleMessage,HeartbeatMsg,HeartbeatRequest,
    HeartbeatReply
}; */


type NodeAddrFn = dyn Fn(usize) -> String + Send + Sync;

#[derive(Debug)]
struct ConnectionPool {
    connections: ArrayQueue<RpcClient<tonic::transport::Channel>>,
}

struct Connection {
    conn: RpcClient<tonic::transport::Channel>,
    pool: Arc<ConnectionPool>,
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.pool.replenish(self.conn.clone())
    }
}


impl ConnectionPool {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            connections: ArrayQueue::new(16),
        })
    }

    async fn connection<S: ToString>(&self, addr: S) -> RpcClient<tonic::transport::Channel> {
        let addr = addr.to_string();
        match self.connections.pop() {
            Some(x) => x,
            None => RpcClient::connect(addr).await.unwrap(),
        }
    }

    fn replenish(&self, conn: RpcClient<tonic::transport::Channel>) {
        let _ = self.connections.push(conn);
    }
}

#[derive(Debug, Clone)]
struct Connections(Arc<Mutex<HashMap<String, Arc<ConnectionPool>>>>);

impl Connections {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(HashMap::new())))
    }

    async fn connection<S: ToString>(&self, addr: S) -> Connection {
        let mut conns = self.0.lock().await;
        let addr = addr.to_string();
        let pool = conns
            .entry(addr.clone())
            .or_insert_with(ConnectionPool::new);
        Connection {
            conn: pool.connection(addr).await,
            pool: pool.clone(),
        }
    }
}

/// RPC transport.
#[derive(Derivative)]
#[derivative(Debug)]
pub struct RpcTransport {
    /// Node address mapping function.
    #[derivative(Debug = "ignore")]
    node_addr: Box<NodeAddrFn>,
    connections: Connections,
}

impl RpcTransport {
    /// Creates a new RPC transport.
    pub fn new(node_addr: Box<NodeAddrFn>) -> Self {
        RpcTransport {
            node_addr,
            connections: Connections::new(),
        }
    }
}



#[async_trait]
impl StoreTransport for RpcTransport {
    
    fn SendSeqPaxosMsg(&self, msg: Message<StoreCommand,()>) {

        let message=SeqpaxosToRpc(msg.clone());
        let peer = (self.node_addr)(msg.to as usize);
        let pool = self.connections.clone();

        tokio::task::spawn(async move {//concurrent task execution
            let mut client = pool.connection(peer).await;
            let request = tonic::Request::new(message);
            client.conn.handle_sp_message(request).await.unwrap();//give to servrer as sequence paxos form
        });

    }

    fn SendBleMessage(&self, msg: BLEMessage) {
        let message = BleMessagepr(msg.clone());
        let peer = (self.node_addr)(msg.to as usize);
        let pool = self.connections.clone();
        tokio::task::spawn(async move {
            let mut client = pool.connection(peer).await;
            let request = tonic::Request::new(message);
            client.conn.handle_ble_message(request).await.unwrap();//give to servrer as ble message form
        });
    } 
}













        /* match msg {
            Message::Promise{
                n,
                n_accepted,
                sync_item,
                ld,
                la,
                ss,
            }=>{
                let n=n as Ballot;
                let n_accepted=n_accepted as Ballot;
                let sync_item=sync_item as SyncItem;
                let ld=ld as u64;
                let la=la as u64;
                let ss=ss as StopSign;
                let request = Promise {
                    n,
                n_accepted,
                sync_item,
                    ld,
                    la,
                    ss,
                };
                let peer = (self.node_addr)(to_id);
                let pool = self.connections.clone();
                tokio::task::spawn(async move {
                    let mut client = pool.connection(peer).await;
                    let request = tonic::Request::new(request.clone());
                    client.conn.append_entries(request).await.unwrap();
                });
            }
            Message::Prepare{
                n,
                ld,
                n_accepted,
                la
            }=>{
                let n=n as Ballot;
                let ld=ld as u64;
                let n_accepted=n_accepted as Ballot;

                let request = Prepare {
                    n,
                ld,
                n_accepted,
                    la
                };
                let peer = (self.node_addr)(to_id);
                let pool = self.connections.clone();
                tokio::task::spawn(async move {
                    let mut client = pool.connection(peer).await;
                    let request = tonic::Request::new(request.clone());
                    client.conn.append_entries(request).await.unwrap();
                });
            }

            Message::AcceptSync{
                n,
                sync_item,
                sync_idx,
                decide_idx,
                stopSign

            }=>{
                let n=n as Ballot;
                let sync_item=sync_item as SyncItem;
                let decide_idx=decide_idx as u64;
                let stopSign=stopSign as StopSign;
                let request = AcceptSync {
                    n,
                    sync_item,
                    sync_idx,
                    decide_idx,
                    stopSign
                };
                let peer = (self.node_addr)(to_id);
                let pool = self.connections.clone();
                tokio::task::spawn(async move {
                    let mut client = pool.connection(peer).await;
                    let request = tonic::Request::new(request.clone());
                    client.conn.append_entries(request).await.unwrap();
                });
            } */





            /*
            Message::AppendEntryRequest {
                from_id,
                term,
                prev_log_index,
                prev_log_term,
                entries,
                commit_index,
            } => {
                let from_id = from_id as u64;
                let term = term as u64;
                let prev_log_index = prev_log_index as u64;
                let prev_log_term = prev_log_term as u64;
                let entries = entries
                    .iter()
                    .map(|entry| {
                        let id = entry.transition.id as u64;
                        let index = entry.index as u64;
                        let sql = entry.transition.sql.clone();
                        let term = entry.term as u64;
                        LogEntry {
                            id,
                            sql,
                            index,
                            term,
                        }
                    })
                    .collect();
                let commit_index = commit_index as u64;
                let request = AppendEntriesRequest {
                    from_id,
                    term,
                    prev_log_index,
                    prev_log_term,
                    entries,
                    commit_index,
                };
                let peer = (self.node_addr)(to_id);
                let pool = self.connections.clone();
                tokio::task::spawn(async move {
                    let mut client = pool.connection(peer).await;
                    let request = tonic::Request::new(request.clone());
                    client.conn.append_entries(request).await.unwrap();
                });
            }
            Message::AppendEntryResponse {
                from_id,
                term,
                success,
                last_index,
                mismatch_index,
            } => {
                let from_id = from_id as u64;
                let term = term as u64;
                let last_index = last_index as u64;
                let mismatch_index = mismatch_index.map(|idx| idx as u64);
                let request = AppendEntriesResponse {
                    from_id,
                    term,
                    success,
                    last_index,
                    mismatch_index,
                };
                let peer = (self.node_addr)(to_id);
                let pool = self.connections.clone();
                tokio::task::spawn(async move {
                    let mut client = pool.connection(peer).await;
                    let request = tonic::Request::new(request.clone());
                    client
                        .conn
                        .respond_to_append_entries(request)
                        .await
                        .unwrap();
                });
            }
            Message::VoteRequest {
                from_id,
                term,
                last_log_index,
                last_log_term,
            } => {
                let from_id = from_id as u64;
                let term = term as u64;
                let last_log_index = last_log_index as u64;
                let last_log_term = last_log_term as u64;
                let request = VoteRequest {
                    from_id,
                    term,
                    last_log_index,
                    last_log_term,
                };
                let peer = (self.node_addr)(to_id);
                let pool = self.connections.clone();
                tokio::task::spawn(async move {
                    let mut client = pool.connection(peer).await;
                    let vote = tonic::Request::new(request.clone());
                    client.conn.vote(vote).await.unwrap();
                });
            }
            Message::VoteResponse {
                from_id,
                term,
                vote_granted,
            } => {
                let peer = (self.node_addr)(to_id);
                tokio::task::spawn(async move {
                    let from_id = from_id as u64;
                    let term = term as u64;
                    let response = VoteResponse {
                        from_id,
                        term,
                        vote_granted,
                    };
                    if let Ok(mut client) = RpcClient::connect(peer.to_string()).await {
                        let response = tonic::Request::new(response.clone());
                        client.respond_to_vote(response).await.unwrap();
                    }
                });
            }*/
        
    

   



/// RPC service.
#[derive(Debug)]
pub struct RpcService {
    /// The ChiselStore server access via this RPC service.
    pub server: Arc<StoreServer<RpcTransport>>,
}

impl RpcService {
    /// Creates a new RPC service.
    pub fn new(server: Arc<StoreServer<RpcTransport>>) -> Self {
        Self { server }
    }
} 



#[tonic::async_trait]
impl Rpc for RpcService {
    async fn execute(
        &self,
        request: Request<Query>,
    ) -> Result<Response<QueryResults>, tonic::Status> {
        let query = request.into_inner();
        /* let consistency =
            proto::Consistency::from_i32(query.consistency).unwrap_or(proto::Consistency::Strong);
        let consistency = match consistency {
            proto::Consistency::Strong => Consistency::Strong,
            proto::Consistency::RelaxedReads => Consistency::RelaxedReads,
        }; */
        let server = self.server.clone();
        let results = match server.query(query.sql).await {
            Ok(results) => results,
            Err(e) => return Err(Status::internal(format!("{}", e))),
        };
        let mut rows = vec![];
        for row in results.rows {
            rows.push(QueryRow {
                values: row.values.clone(),
            })
        }
        Ok(Response::new(QueryResults { rows }))
    }



    // converts rpc message to sequence paxos and gives to server
    async fn handle_sp_message(&self, request: Request<Psg>) -> Result<Response<Void>, tonic::Status>{

        let message = RPCtoSeqPaxos(request.into_inner());
        let server = self.server.clone();
        server.RcvSPMsg(message);
        Ok(Response::new(Void {}))

        
    }

    // converts rpc message to sequence paxos and gives to server

    async fn handle_ble_message(&self, request: Request<BleMessageObj>) -> Result<Response<Void>, tonic::Status> {
        let message = blemessageSP(request.into_inner());
        let server = self.server.clone();
        server.RcvBLEMsg(message);
        Ok(Response::new(Void {}))
    }



}

    /* async fn vote(&self, request: Request<VoteRequest>) -> Result<Response<Void>, tonic::Status> {
        let msg = request.into_inner();
        let from_id = msg.from_id as usize;
        let term = msg.term as usize;
        let last_log_index = msg.last_log_index as usize;
        let last_log_term = msg.last_log_term as usize;
        let msg = little_raft::message::Message::VoteRequest {
            from_id,
            term,
            last_log_index,
            last_log_term,
        };
        let server = self.server.clone();
        server.recv_msg(msg);
        Ok(Response::new(Void {}))
    }

    async fn respond_to_vote(
        &self,
        request: Request<VoteResponse>,
    ) -> Result<Response<Void>, tonic::Status> {
        let msg = request.into_inner();
        let from_id = msg.from_id as usize;
        let term = msg.term as usize;
        let vote_granted = msg.vote_granted;
        let msg = little_raft::message::Message::VoteResponse {
            from_id,
            term,
            vote_granted,
        };
        let server = self.server.clone();
        server.recv_msg(msg);
        Ok(Response::new(Void {}))
    }

    async fn append_entries(
        &self,
        request: Request<AppendEntriesRequest>,
    ) -> Result<Response<Void>, tonic::Status> {
        let msg = request.into_inner();
        let from_id = msg.from_id as usize;
        let term = msg.term as usize;
        let prev_log_index = msg.prev_log_index as usize;
        let prev_log_term = msg.prev_log_term as usize;
        let entries: Vec<little_raft::message::LogEntry<StoreCommand>> = msg
            .entries
            .iter()
            .map(|entry| {
                let id = entry.id as usize;
                let sql = entry.sql.to_string();
                let transition = StoreCommand { id, sql };
                let index = entry.index as usize;
                let term = entry.term as usize;
                little_raft::message::LogEntry {
                    transition,
                    index,
                    term,
                }
            })
            .collect();
        let commit_index = msg.commit_index as usize;
        let msg = little_raft::message::Message::AppendEntryRequest {
            from_id,
            term,
            prev_log_index,
            prev_log_term,
            entries,
            commit_index,
        };
        let server = self.server.clone();
        server.recv_msg(msg);
        Ok(Response::new(Void {}))
    }

    async fn respond_to_append_entries(
        &self,
        request: tonic::Request<AppendEntriesResponse>,
    ) -> Result<tonic::Response<Void>, tonic::Status> {
        let msg = request.into_inner();
        let from_id = msg.from_id as usize;
        let term = msg.term as usize;
        let success = msg.success;
        let last_index = msg.last_index as usize;
        let mismatch_index = msg.mismatch_index.map(|idx| idx as usize);
        let msg = little_raft::message::Message::AppendEntryResponse {
            from_id,
            term,
            success,
            last_index,
            mismatch_index,
        };
        let server = self.server.clone();
        server.recv_msg(msg);
        Ok(Response::new(Void {}))
    }
} */ 


//converting seqpaxos message to rpc
fn SeqpaxosToRpc(message: Message<StoreCommand, ()>) -> Psg {
    Psg {
        from: message.from,
        to: message.to,
        msg: Some(match message.msg {
            PaxosMsg::PrepareReq => psg::Msg::Preparereq(PrepareReqObj {}),
            PaxosMsg::Prepare(prepare) => psg::Msg::Prepare(Preparepr(prepare)),
            PaxosMsg::Promise(promise) => psg::Msg::Promise(Promispr(promise)),
            PaxosMsg::AcceptSync(accept_sync) => psg::Msg::AcceptSync(AcceptSyncpr(accept_sync)),
            PaxosMsg::FirstAccept(first_accept) => psg::Msg::FirstAccept(FirstMessagepr(first_accept)),
            PaxosMsg::AcceptDecide(accept_decide) => psg::Msg::AcceptDecide(AcceptDecidepr(accept_decide)),
            PaxosMsg::Accepted(accepted) => psg::Msg::Accepted(AcceptedPr(accepted)),
            PaxosMsg::Decide(decide) => psg::Msg::Decide(DecidePr(decide)),
            PaxosMsg::ProposalForward(proposals) => psg::Msg::Proposalforward(ProposalForwardpr(proposals)),
            PaxosMsg::Compaction(compaction) => psg::Msg::Compaction(Compactionpr(compaction)),
            PaxosMsg::ForwardCompaction(compaction) => psg::Msg::ForwardCompaction(Compactionpr(compaction)),
            PaxosMsg::AcceptStopSign(accept_stop_sign) => psg::Msg::AcceptStopSign(AcceptStopSignPr(accept_stop_sign)),
            PaxosMsg::AcceptedStopSign(accepted_stop_sign) => psg::Msg::AcceptedStopSign(AcceptedSSpr(accepted_stop_sign)),
            PaxosMsg::DecideStopSign(decide_stop_sign) => psg::Msg::DecideSsSign(DecideSSpr(decide_stop_sign)),
        })
    }
} 

//converting ble message to rpc
fn BleMessagepr(message: BLEMessage) -> BleMessageObj {
    BleMessageObj {
        from: message.from,
        to: message.to,
        msg: Some(match message.msg {
            HeartbeatMsg::Request(request) => ble_message_obj::Msg::Heartbeatreq(HeartBeatRequestpr(request)),
            HeartbeatMsg::Reply(reply) => ble_message_obj::Msg::Heartbeatrep(HeartBeatReplypr(reply)),
        })
    }
}


fn ballotpr(ballot: Ballot) -> BallotObj {
    BallotObj {
        n: ballot.n,
        priority: ballot.priority,
        pid: ballot.pid,
    }
}

//converting prepare message to rpc message
fn Preparepr(obj: Prepare) -> PrepareObj {
    PrepareObj {
        n: Some(ballotpr(obj.n)),
        ld: obj.ld,
        n_accepted: Some(ballotpr(obj.n_accepted)),
        la: obj.la,
    }
}


fn StoreCommandpr(cmd: StoreCommand) -> StoreCommandObj {
    StoreCommandObj {
        id: cmd.id as u64,
        query: cmd.sql.clone()
    }
}


fn SyncItempr(sync_item: SyncItem<StoreCommand, ()>) -> SyncItemObj {
    SyncItemObj {
        inum: Some(match sync_item {
            SyncItem::Entries(vec) => sync_item_obj::Inum::E(sync_item_obj::Entries { vec: vec.into_iter().map(|e| StoreCommandpr(e)).collect() }),
            SyncItem::Snapshot(ss) => match ss {
                SnapshotType::Complete(_) => sync_item_obj::Inum::S(sync_item_obj::SnapshotType::Complete as i32),
                SnapshotType::Delta(_) => sync_item_obj::Inum::S(sync_item_obj::SnapshotType::Delta as i32),
                SnapshotType::_Phantom(_) => sync_item_obj::Inum::S(sync_item_obj::SnapshotType::Phantom as i32),
            },
            SyncItem::None => sync_item_obj::Inum::None(sync_item_obj::None {}),
        }),
    }
}



fn SSpr(stop_sign: StopSign) -> StopSignObj {
    StopSignObj {
        config_id: stop_sign.config_id,
        nodes: stop_sign.nodes,
        metadata: match stop_sign.metadata {
            Some(vec) => Some(stop_sign_obj::Metadata { vec: vec.into_iter().map(|m| m as u32).collect() }),
            None => None,
        }
    }
}
//converting promise message to rpc message

fn Promispr(promise: Promise<StoreCommand, ()>) -> PromiseObj {
    PromiseObj {
        n: Some(ballotpr(promise.n)),
        n_accepted: Some(ballotpr(promise.n_accepted)),
        sync_item: match promise.sync_item {
            Some(s) => Some(SyncItempr(s)),
            None => None,
        },
        ld: promise.ld,
        la: promise.la,
        stop_sign: match promise.stopsign {
            Some(ss) => Some(SSpr(ss)),
            None => None,
        },
    }
}

//converting acceptsync message to rpc message
fn AcceptSyncpr(accept_sync: AcceptSync<StoreCommand, ()>) -> AcceptSyncObj {
    AcceptSyncObj {
        n: Some(ballotpr(accept_sync.n)),
        sync_item: Some(SyncItempr(accept_sync.sync_item)),
        sync_idx: accept_sync.sync_idx,
        decide_idx: accept_sync.decide_idx,
        stop_sign: match accept_sync.stopsign {
            Some(ss) => Some(SSpr(ss)),
            None => None,
        },
    }
}

//converting firstaccept message to rpc message
fn FirstMessagepr(first_accept: FirstAccept<StoreCommand>) -> FirstMessageObj {
    FirstMessageObj {
        n: Some(ballotpr(first_accept.n)),
        e: first_accept.entries.into_iter().map(|e| StoreCommandpr(e)).collect(),
    }
}


//converting acceptdecide message to rpc message

fn AcceptDecidepr(accept_decide: AcceptDecide<StoreCommand>) -> AcceptDecideObj {
    AcceptDecideObj {
        n: Some(ballotpr(accept_decide.n)),
        ld: accept_decide.ld,
        e: accept_decide.entries.into_iter().map(|e| StoreCommandpr(e)).collect(),
    }
}


//converting accepted message to rpc message
fn AcceptedPr(accepted: Accepted) -> AcceptedObj {
    AcceptedObj {
        n: Some(ballotpr(accepted.n)),
        la: accepted.la,
    }
}

//converting decided message to rpc message
fn DecidePr(decide: Decide) -> DecideObj {
    DecideObj {
        n: Some(ballotpr(decide.n)),
        ld: decide.ld,
    }
}

//converting proposal forward message to rpc message
fn ProposalForwardpr(proposals:Vec<StoreCommand>) -> ProposalForwardObj {
    ProposalForwardObj {
        entries: proposals.into_iter().map(|e| StoreCommandpr(e)).collect(),
    }
}

//converting compaction message to rpc message
fn Compactionpr(compaction: Compaction) -> CompactionObj {
    CompactionObj {
        compaction: Some(match compaction {
            Compaction::Trim(trim) => compaction_obj::Compaction::Trim(compaction_obj::Trim { trim }),
            Compaction::Snapshot(ss) => compaction_obj::Compaction::Snapshot(ss),
        }),
    }
}


//converting acceptstopsign message to rpc message
fn AcceptStopSignPr(accept_stop_sign: AcceptStopSign) -> AcceptSsObj {
    AcceptSsObj {
        n: Some(ballotpr(accept_stop_sign.n)),
        ss: Some(SSpr(accept_stop_sign.ss)),
    }
}


//converting acceptedstopsign message to rpc message
fn AcceptedSSpr(accepted_stop_sign: AcceptedStopSign) -> AcceptedSsObj {
    AcceptedSsObj {
        n: Some(ballotpr(accepted_stop_sign.n)),
    }
}


//converting decidedss message to rpc message
fn DecideSSpr(decide_stop_sign: DecideStopSign) -> DecidedSsObj {
    DecidedSsObj {
        n: Some(ballotpr(decide_stop_sign.n)),
    }
}


//converting heartbeat requesr message to rpc message
fn HeartBeatRequestpr(heartbeat_request: HeartbeatRequest) -> HeartbeatRequestObj {
    HeartbeatRequestObj {
        round: heartbeat_request.round,
    }
}

//converting heartbeat reply message to rpc message
fn HeartBeatReplypr(heartbeat_reply: HeartbeatReply) -> HeartbeatReplyObj {
    HeartbeatReplyObj {
        round: heartbeat_reply.round,
        ballot: Some(ballotpr(heartbeat_reply.ballot)),
        majority_connected: heartbeat_reply.majority_connected,
    }
}


fn RPCtoSeqPaxos(obj: Psg)->Message<StoreCommand, ()> {

    Message {
        from: obj.from,
        to: obj.to,
        msg: match obj.msg.unwrap() {
            psg::Msg::Preparereq(_) => PaxosMsg::PrepareReq,
            psg::Msg::Prepare(prepare) => PaxosMsg::Prepare(PrepareSP(prepare)),
            psg::Msg::Promise(promise) => PaxosMsg::Promise(Promisesp(promise)),
            psg::Msg::AcceptSync(accept_sync) => PaxosMsg::AcceptSync(AcceptSyncsp(accept_sync)),
            psg::Msg::FirstAccept(first_accept) => PaxosMsg::FirstAccept(FirstMessagesp(first_accept)),
            psg::Msg::AcceptDecide(accept_decide) => PaxosMsg::AcceptDecide(AcceptDecidesp(accept_decide)),
            psg::Msg::Accepted(accepted) => PaxosMsg::Accepted(Acceptedsp(accepted)),
            psg::Msg::Decide(decide) => PaxosMsg::Decide(Decidesp(decide)),
            psg::Msg::Proposalforward(proposals) => PaxosMsg::ProposalForward(ProposalForwardsp(proposals)),
            psg::Msg::Compaction(compaction) => PaxosMsg::Compaction(Compactionsp(compaction)),
            psg::Msg::ForwardCompaction(compaction) => PaxosMsg::ForwardCompaction(Compactionsp(compaction)),
            psg::Msg::AcceptStopSign(accept_stop_sign) => PaxosMsg::AcceptStopSign(AcceptSSsp(accept_stop_sign)),
            psg::Msg::AcceptedStopSign(accepted_stop_sign) => PaxosMsg::AcceptedStopSign(AcceptedSSsp(accepted_stop_sign)),
            psg::Msg::DecideSsSign(decide_stop_sign) => PaxosMsg::DecideStopSign(DecideSSsp(decide_stop_sign)),
        }
    }


}

//converting blemessage to sequence paxos



fn blemessageSP(obj: BleMessageObj) -> BLEMessage {
    BLEMessage {
        from: obj.from,
        to: obj.to,
        msg: match obj.msg.unwrap() {
            ble_message_obj::Msg::Heartbeatreq(request) => HeartbeatMsg::Request(HeartbeatRequestsp(request)),
            ble_message_obj::Msg::Heartbeatrep(reply) => HeartbeatMsg::Reply(HeartBeatReplysp(reply)),
        }
    }
}


fn ballotsp(obj:BallotObj)->Ballot{
    Ballot{
        n:obj.n,
        priority:obj.priority,
        pid:obj.pid,
    }
}



//converting prepare message to sequence paxos
fn PrepareSP(obj: PrepareObj) -> Prepare {
    Prepare {
        n: ballotsp(obj.n.unwrap()),
        ld: obj.ld,
        n_accepted: ballotsp(obj.n_accepted.unwrap()),
        la: obj.la,
    }
}



fn StoreCommandSP(obj: StoreCommandObj) -> StoreCommand {
    StoreCommand {
        id: obj.id as usize,
        sql: obj.query.clone()
    }
}


fn SyncItemSP(obj: SyncItemObj) -> SyncItem<StoreCommand, ()> {
    match obj.inum.unwrap() {
        sync_item_obj::Inum::E(entries) => SyncItem::Entries(entries.vec.into_iter().map(|e| StoreCommandSP(e)).collect()),
        sync_item_obj::Inum::S(ss) => match sync_item_obj::SnapshotType::from_i32(ss) {
            Some(sync_item_obj::SnapshotType::Complete) => SyncItem::Snapshot(SnapshotType::Complete(())),
            Some(sync_item_obj::SnapshotType::Delta) => SyncItem::Snapshot(SnapshotType::Delta(())),
            Some(sync_item_obj::SnapshotType::Phantom) => SyncItem::Snapshot(SnapshotType::_Phantom(PhantomData)),
            _ => unimplemented!() // todo: verify
        },
        sync_item_obj::Inum::None(_) => SyncItem::None,

    }
}


fn SSsp(obj: StopSignObj) -> StopSign {
    StopSign {
        config_id: obj.config_id,
        nodes: obj.nodes,
        metadata: match obj.metadata {
            Some(md) => Some(md.vec.into_iter().map(|m| m as u8).collect()),
            None => None,
        },
    }
}


//converting promise message to sequence paxos

fn Promisesp(obj: PromiseObj) -> Promise<StoreCommand, ()> {
    Promise {
        n: ballotsp(obj.n.unwrap()),
        n_accepted: ballotsp(obj.n_accepted.unwrap()),
        sync_item: match obj.sync_item {
            Some(s) => Some(SyncItemSP(s)),
            None => None,
        },
        ld: obj.ld,
        la: obj.la,
        stopsign: match obj.stop_sign {
            Some(ss) => Some(SSsp(ss)),
            None => None,
        },
    }
}



//converting accept sync message to sequence paxos
fn AcceptSyncsp(obj: AcceptSyncObj) -> AcceptSync<StoreCommand, ()> {
    AcceptSync {
        n: ballotsp(obj.n.unwrap()),
        sync_item: SyncItemSP(obj.sync_item.unwrap()),
        sync_idx: obj.sync_idx,
        decide_idx: obj.decide_idx,
        stopsign: match obj.stop_sign {
            Some(ss) => Some(SSsp(ss)),
            None => None,
        },
    }
}

//converting first accept message to sequence paxos
fn FirstMessagesp(obj: FirstMessageObj) -> FirstAccept<StoreCommand> {
    FirstAccept {
        n: ballotsp(obj.n.unwrap()),
        entries: obj.e.into_iter().map(|e| StoreCommandSP(e)).collect(),
    }
}


//

fn AcceptDecidesp(obj: AcceptDecideObj) -> AcceptDecide<StoreCommand> {
    AcceptDecide {
        n: ballotsp(obj.n.unwrap()),
        ld: obj.ld,
        entries: obj.e.into_iter().map(|e| StoreCommandSP(e)).collect(),
    }
}

//converting accepted message to sequence paxos
fn Acceptedsp(obj: AcceptedObj) -> Accepted {
    Accepted {
        n: ballotsp(obj.n.unwrap()),
        la: obj.la,
    }
}

//converting decide message to sequence paxos
fn Decidesp(obj: DecideObj) -> Decide {
    Decide {
        n: ballotsp(obj.n.unwrap()),
        ld: obj.ld,
    }
}

//converting proposal forward message to sequence paxos
fn ProposalForwardsp(obj: ProposalForwardObj) -> Vec<StoreCommand> {
    obj.entries.into_iter().map(|e| StoreCommandSP(e)).collect()
}


//converting compaction message to sequence paxos
fn Compactionsp(obj: CompactionObj) -> Compaction {
    match obj.compaction.unwrap() {
        compaction_obj::Compaction::Trim(trim) => Compaction::Trim(trim.trim),
        compaction_obj::Compaction::Snapshot(ss) => Compaction::Snapshot(ss),
    }
}


//converting accept stop sign message to sequence paxos
fn AcceptSSsp(obj: AcceptSsObj) -> AcceptStopSign {
    AcceptStopSign {
        n: ballotsp(obj.n.unwrap()),
        ss: SSsp(obj.ss.unwrap()),
    }
}


// converting accepted stop sign to sequence paxos
fn AcceptedSSsp(obj: AcceptedSsObj) -> AcceptedStopSign {
    AcceptedStopSign {
        n: ballotsp(obj.n.unwrap()),
    }
}

//converting decide message to sequence paxos
fn DecideSSsp(obj: DecidedSsObj) -> DecideStopSign {
    DecideStopSign {
        n: ballotsp(obj.n.unwrap()),
    }
}


//convering heartbeat request message to sequence paxos
fn HeartbeatRequestsp(obj: HeartbeatRequestObj) -> HeartbeatRequest {
    HeartbeatRequest {
        round: obj.round,
    }
}


//converting heart beat reply message to sequence paxos
fn HeartBeatReplysp(obj: HeartbeatReplyObj) -> HeartbeatReply {
    HeartbeatReply {
        round: obj.round,
        ballot: ballotsp(obj.ballot.unwrap()),
        majority_connected: obj.majority_connected,
    }
}