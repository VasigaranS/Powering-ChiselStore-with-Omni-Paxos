//! ChiselStore server module.
use crate::errors::StoreError;
use async_notify::Notify;
use async_trait::async_trait;
use crossbeam_channel as channel;
use crossbeam_channel::{Receiver, Sender};
use derivative::Derivative;
use sqlite::{Connection, OpenFlags};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::Duration;
// importing omni paxos core
use omnipaxos_core::messages::Message;
use omnipaxos_core::ballot_leader_election::{Ballot, BallotLeaderElection, BLEConfig};
use omnipaxos_core::ballot_leader_election::messages::BLEMessage;
use omnipaxos_core::sequence_paxos::{SequencePaxos, SequencePaxosConfig};
use omnipaxos_core::storage::{Snapshot, StopSignEntry, Storage};

/// ChiselStore transport layer.
///
/// Your application should implement this trait to provide network access
/// to the ChiselStore server.
#[async_trait]
pub trait StoreTransport {
    /// used to send a command as sequence paxos message  through transport
    fn SendSeqPaxosMsg(&self, msg: Message<StoreCommand, ()>);
    // used to send a ble message through rpc traspport
    fn SendBleMessage(&self, msg: BLEMessage);
}

/// Store command.
///
/// A store command is a SQL statement that is replicated in the Raft cluster and store command has instances of  sql entry to a log
#[derive(Clone, Debug)]
pub struct StoreCommand {
    /// Unique ID of this command.
    pub id: usize,
    /// The SQL statement of this command.
    pub sql: String,
}


#[derive(Debug)]
pub struct QueryResultsHandler {
    q_notifiers: HashMap<u64, Arc<Notify>>,
    q_results: HashMap<u64, Result<QueryResults, StoreError>>,
}




/// Store configuration.
#[derive(Debug)]
struct StoreConfig {
    /// Connection pool size.
    conn_pool_size: usize,
    query_results: Arc<Mutex<QueryResultsHandler>>,
}

#[derive(Derivative)]
#[derivative(Debug)]
//this contains an instance of sql store but this itself is the sequence paxos log 
struct Store<S> where S: Snapshot<StoreCommand> { // where S: Snapshot<StoreCommand>
    /// ID of the node this Cluster objects is on.
    id: u64,
    // Vector which contains all the logged entries in-memory.
    log: Vec<StoreCommand>,
    /// Last promised round.
    n_prom: Ballot,
    /// Last accepted round.
    acc_round: Ballot,
    /// Length of the decided log.
    ld: u64,
    /// Garbage collected index.
    trimmed_idx: u64,
    /// Stored snapshot
    snapshot: Option<S>,
    /// Stored StopSign
    stopsign: Option<StopSignEntry>,
    // stuff for sql 
    #[derivative(Debug = "ignore")]
    conn_pool: Vec<Arc<Mutex<Connection>>>,
    conn_idx: usize,
    pending_transitions: Vec<StoreCommand>,
    // command_completions: HashMap<u64, Arc<Notify>>,
    // storing results
    query_results: Arc<Mutex<QueryResultsHandler>>,
}

impl<S> Store<S> where S: Snapshot<StoreCommand> {
    pub fn new(id: u64, config: StoreConfig) -> Self {
        let mut conn_pool = vec![];
        let conn_pool_size = config.conn_pool_size;
        for _ in 0..conn_pool_size {
            let flags = OpenFlags::new()
                .set_read_write()
                .set_create()
                .set_no_mutex();
            let mut conn =
                Connection::open_with_flags(format!("node{}.db", id), flags).unwrap();
            conn.set_busy_timeout(5000).unwrap();
            conn_pool.push(Arc::new(Mutex::new(conn)));
        }
        let conn_idx = 0;
        Store {
            id,
            log: vec![],
            n_prom: Ballot::default(),
            acc_round: Ballot::default(),
            ld: 0,
            trimmed_idx: 0,
            snapshot: None,
            stopsign: None,
            conn_pool,
            conn_idx,
            pending_transitions: Vec::new(),
            query_results: config.query_results
        }
    }

    pub fn get_connection(&mut self) -> Arc<Mutex<Connection>> {
        let idx = self.conn_idx % self.conn_pool.len();
        let conn = &self.conn_pool[idx];
        self.conn_idx += 1;
        conn.clone()
    }
}



impl QueryResultsHandler {
    fn default() -> Self {// implemented at the 'start' of the storeserver
        Self { q_notifiers: HashMap::new(), q_results: HashMap::new()}
    }
    pub fn add_notifier(&mut self, id: u64, notifier: Arc<Notify>) {//lnotifier added in query async function 
        self.q_notifiers.insert(id, notifier);
    }

    pub fn add_result(&mut self, id: u64, result: Result<QueryResults, StoreError>) {// stores resulst of queries called while executing queries in set_decided_idx fun
        if let Some(completion) = self.q_notifiers.remove(&(id)) {
            self.q_results.insert(id, result);
            completion.notify();
        }
    }

    pub fn remove_result(&mut self, id: &u64) -> Option<Result<QueryResults, StoreError>> {
        self.q_results.remove(id)
    }
}

// This is storage imported from omni paxos. Ans is the storage for the omnipaxos 
// we have utility functiosn to append the log , retrieve log to and from this storage.
impl<S> Storage<StoreCommand, S> for Store<S> where S: Snapshot<StoreCommand>
{
     /// Appends an entry to the end of the log and returns the log length.
    fn append_entry(&mut self, entry: StoreCommand) -> u64 {
        self.log.push(entry);
        self.get_log_len()
    }

    /// Appends the entries of `entries` to the end of the log and returns the log length.

    fn append_entries(&mut self, entries: Vec<StoreCommand>) -> u64 {
        let mut e = entries;
        self.log.append(&mut e);
        self.get_log_len()
    }

    /// Appends the entries of `entries` to the prefix from index `from_index` in the log and returns the log length.
    fn append_on_prefix(&mut self, from_idx: u64, entries: Vec<StoreCommand>) -> u64 {
        self.log.truncate(from_idx as usize);
        self.append_entries(entries)
    }

    /// Sets the round that has been promised.
    fn set_promise(&mut self, n_prom: Ballot) {
        self.n_prom = n_prom;
    }

    /// Sets the decided index in the log and executes the query
    fn set_decided_idx(&mut self, ld: u64) {
        
        let runqueries = self.log[(self.ld as usize)..(ld as usize)].to_vec();
        for q in runqueries.iter() {
            let conn = self.get_connection();
            let results = query(conn, q.sql.clone());

            let mut query_results = self.query_results.lock().unwrap();
            query_results.add_result(q.id as u64, results);
        }
        self.ld = ld;
    }


    /// Returns the decided index in the log.
    fn get_decided_idx(&self) -> u64 {
        self.ld
    }

    /// Sets the latest accepted round.

    fn set_accepted_round(&mut self, na: Ballot) {
        self.acc_round = na;
    }
    /// Returns the latest round in which entries have been accepted.
    fn get_accepted_round(&self) -> Ballot {
        self.acc_round
    }
    /// Returns the entries in the log in the index interval of [from, to)
    fn get_entries(&self, from: u64, to: u64) -> &[StoreCommand] {
        self.log.get(from as usize..to as usize).unwrap_or(&[])
    }
    /// Returns the current length of the log.
    fn get_log_len(&self) -> u64 {
        self.log.len() as u64
    }
    /// Returns the suffix of entries in the log from index `from`.
    fn get_suffix(&self, from: u64) -> &[StoreCommand] {
        match self.log.get(from as usize..) {
            Some(s) => s,
            None => &[],
        }
    }
    /// Returns the round that has been promised.
    fn get_promise(&self) -> Ballot {
        self.n_prom
    }
    /// Sets the StopSign used for reconfiguration.
    fn set_stopsign(&mut self, s: StopSignEntry) {
        self.stopsign = Some(s);
    }
    /// Returns the stored StopSign.
    fn get_stopsign(&self) -> Option<StopSignEntry> {
        self.stopsign.clone()
    }
    /// Removes elements up to the given [`idx`] from storage.
    fn trim(&mut self, trimmed_idx: u64) {
        self.log.drain(0..trimmed_idx as usize);
    }
    /// Sets the compacted (i.e. trimmed or snapshotted) index.
    fn set_compacted_idx(&mut self, trimmed_idx: u64) {
        self.trimmed_idx = trimmed_idx;
    }
    /// Returns the garbage collector index from storage.
    fn get_compacted_idx(&self) -> u64 {
        self.trimmed_idx
    }
    /// Sets the snapshot.
    fn set_snapshot(&mut self, snapshot: S) {
        self.snapshot = Some(snapshot);
    }
    /// Returns the stored snapshot.
    fn get_snapshot(&self) -> Option<S> {
        self.snapshot.clone()
    }
}

// this is to iterate and do sql query execution . done when set_decided_idx function is done at omni paxos storage and returns the query results
fn query(conn: Arc<Mutex<Connection>>, sql: String) -> Result<QueryResults, StoreError> {
    let conn = conn.lock().unwrap();
    let mut rows = vec![];
    conn.iterate(sql, |pairs| {
        let mut row = QueryRow::new();
        for &(_, value) in pairs.iter() {
            row.values.push(value.unwrap().to_string());
        }
        rows.push(row);
        true
    })?;
    Ok(QueryResults { rows })
}

/// ChiselStore server.
#[derive(Derivative)]
#[derivative(Debug)]
pub struct StoreServer<T: StoreTransport + Send + Sync> {
    id: u64,
    command_id: AtomicU64,
    query_results: Arc<Mutex<QueryResultsHandler>>,
    #[derivative(Debug = "ignore")]
    sp: Arc<Mutex<SequencePaxos<StoreCommand, (), Store<()>>>>,
    #[derivative(Debug = "ignore")]
    ble: Arc<Mutex<BallotLeaderElection>>,
    sp_notifier_rx: Receiver<Message<StoreCommand, ()>>,
    sp_notifier_tx: Sender<Message<StoreCommand, ()>>,
    ble_notifier_rx: Receiver<BLEMessage>,
    ble_notifier_tx: Sender<BLEMessage>,
    transport: T,
}

/// Query row.
#[derive(Debug)]
pub struct QueryRow {
    /// each Column values of the row.
    pub values: Vec<String>,
}

impl QueryRow {
    fn new() -> Self {
        QueryRow { values: Vec::new() }
    }
}

/// Query results.
#[derive(Debug)]
pub struct QueryResults {
    /// Query result rows.
    pub rows: Vec<QueryRow>,
}

impl<T: StoreTransport + Send + Sync> StoreServer<T> {
    /// Start a new server as part of a ChiselStore cluster.
    pub fn start(id: usize, peers: Vec<usize>, transport: T) -> Result<Self, StoreError> {

        let id = id as u64;
        let peers: Vec<u64> = peers.into_iter().map(|p| p as u64).collect();
        let command_id = AtomicU64::new(0);

        //  instance of sequence paxos
        let configuration_id = 0;
        let mut sp_config = SequencePaxosConfig::default();
        sp_config.set_configuration_id(configuration_id);
        sp_config.set_pid(id);
        sp_config.set_peers(peers.to_vec()); 

        let query_results = Arc::new(Mutex::new(QueryResultsHandler::default()));
        let config = StoreConfig{ 
            conn_pool_size: 20, 
            query_results: query_results.clone() 
        };
        let store = Store::new(id, config);
        let sp = Arc::new(Mutex::new(SequencePaxos::with(sp_config, store)));

        // //  instance of ble
        let mut ble_config = BLEConfig::default();
        ble_config.set_pid(id);
        ble_config.set_peers(peers);
        ble_config.set_hb_delay(1000); // from omni paxos documentation, configure it yourself
        let ble = Arc::new(Mutex::new(BallotLeaderElection::with(ble_config)));

        // transport crossbeam_channel
        let (sp_notifier_tx, sp_notifier_rx) = channel::unbounded();
        let (ble_notifier_tx, ble_notifier_rx) = channel::unbounded();

        Ok(StoreServer {
            id,
            command_id,
            query_results,
            sp,
            ble,
            sp_notifier_rx,
            sp_notifier_tx,
            ble_notifier_rx,
            ble_notifier_tx,
            transport,
        })
    }

    /// Run the blocking event loop.
    pub fn run(&self) {
        loop {
            let mut sp = self.sp.lock().unwrap();
            let mut ble = self.ble.lock().unwrap();

            if let Some(leader) = ble.tick() {
                sp.handle_leader(leader);// omni paxos function to handle leader 
            }

            // this to handle any incoming sp messages
            match self.sp_notifier_rx.try_recv() {
                Ok(msg) => {
                    sp.handle(msg);
                },
                _ => {}
            };

            // this to handle any incoming sp messages

            match self.ble_notifier_rx.try_recv() {
                Ok(msg) => {
                    ble.handle(msg);
                },
                _ => {}
            };

            // this to handle outgoing seqpaxos  messages and they send the sp message to rpc after conversion to rpc
            for msg in sp.get_outgoing_msgs() {
                let receiver = msg.to;
                self.transport.SendSeqPaxosMsg(msg);
            }
            // this to handle outgoing ble  messages and they send the ble message to rpc after conversion to rpc
            for msg in ble.get_outgoing_msgs() {
                let receiver = msg.to;
                self.transport.SendBleMessage(msg);
            }

            sleep(Duration::from_millis(1));//loop delay
        }
    }

    //gets called on executing query from the rpc 

    pub async fn query<S: AsRef<str>>(&self, sql_statement: S) -> Result<QueryResults, StoreError> {
        
        let results = {
            let (notify, id) = {
                let id = self.command_id.fetch_add(1, Ordering::SeqCst);
                let command = StoreCommand {
                    id: id as usize,
                    sql: sql_statement.as_ref().to_string()
                };

                let notify = Arc::new(Notify::new());
                self.query_results.lock().unwrap().add_notifier(id, notify.clone());
                self.sp.lock().unwrap().append(command).expect("Failed to append");

                (notify, id)
            };

            notify.notified().await;
            let results = self.query_results.lock().unwrap().remove_result(&id).unwrap();

            results?
        };
        Ok(results)
    }
    
    /// Receive a sequence paxos message from the cluster.
    pub fn RcvSPMsg(&self, msg: Message<StoreCommand, ()>) {
        self.sp_notifier_tx.send(msg).unwrap();
    }

    /// Receive a ballot leader election message from the cluster.
    pub fn RcvBLEMsg(&self, msg: BLEMessage) {
        self.ble_notifier_tx.send(msg).unwrap();
    }
}