enum MasterState {
    NotReady,
    Ready { ex: String }, // After prepare-ex
}
