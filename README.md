# Powering-ChiselStore-with-Omni-Paxos
Omni-Paxos is a replicated log library of sequence consensus similar to Raft. It is used to build consistent services such as replicated state machines. It provides increased resilience and performance compared to raft.Omni-Paxos differs from Raft in the ballot leader election. In Raft ,a server must have an up to date updated log to become a leader. This condition is relaxed in Omni-Paxos , any server can become the leader. The log gets synchronised during the prepare phase.
The project aims at replacing Raft with Omnipaxos in an Open source system called Chiselstore. Chiselstore is a distributed SQLite which is powered by Raft consensus algorithm implemented in Rust language. Replacing Raft with 0mnipaxos changes the sequence consensus algorithm and involves removing the raft dependency and replacing it with Omnipaxos core.
