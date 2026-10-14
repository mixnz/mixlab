//! The ways a job's row and its life can fail — what `jobs` reports.
//!
//! One of the enums [`crate::Error`] wraps transparently, so its messages and sources are
//! the ones a caller of this crate always saw — roadmap task **T207j**.

/// The ways a job's row and its life can fail.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum JobError {
    /// Something tried to move a job that is already over.
    ///
    /// **The ordinary case rather than a corruption**, which is why it is its own variant and not an
    /// [`Error::IllegalJobTransition`]: a job's work is a cancellable task, so a producer that was
    /// cancelled mid-download and reports its last progress on the way out arrives here, and so does
    /// the work finishing at the same instant a `job.cancel` lands. What the caller is told is which
    /// ending got there first, because that is the one that stands.
    ///
    /// [`Error::IllegalJobTransition`]: crate::JobError::IllegalJobTransition
    #[error("job #{job} has already {state}")]
    JobEnded {
        /// The job.
        job: i64,
        /// What it ended as.
        state: mixengine_proto::JobState,
    },

    /// A job was asked to make a move its state machine does not have.
    ///
    /// A bug in the caller, on [`ServiceError::IllegalTransition`]'s own terms — and unreachable while
    /// every ending is reachable from `running`, which is why it is asserted rather than assumed.
    ///
    /// [`ServiceError::IllegalTransition`]: crate::ServiceError::IllegalTransition
    #[error("job #{job} cannot go from {from} to {to}")]
    IllegalJobTransition {
        /// The job.
        job: i64,
        /// Where it is.
        from: mixengine_proto::JobState,
        /// Where the caller wanted it.
        to: mixengine_proto::JobState,
    },

    /// A `jobs` row holds a state this build does not recognise.
    ///
    /// Unreachable through our own writes — the column is `CHECK`ed against the same closed list
    /// [`mixengine_proto::JobState`] is — so it means a database edited by hand, or one written by a
    /// version that knew a state this one does not. [`ServiceError::UnknownServiceState`]'s sibling.
    ///
    /// [`ServiceError::UnknownServiceState`]: crate::ServiceError::UnknownServiceState
    #[error("the state of job #{job} is stored as {value}, which is not a job state")]
    UnknownJobState {
        /// The job whose row cannot be read.
        job: i64,
        /// The word that is in the column.
        value: String,
    },

    /// A `jobs` row holds a kind that is not a name.
    ///
    /// `jobs.kind` is deliberately not `CHECK`ed — the set grows with every phase and every
    /// extension — so unlike the state, this one has no constraint behind it and the reader is the
    /// only thing that refuses.
    #[error("the kind of job #{job} is stored as {value}, which is not a job kind")]
    UnknownJobKind {
        /// The job whose row cannot be read.
        job: i64,
        /// The word that is in the column.
        value: String,
    },

    /// A `jobs` column holds a document that does not parse.
    ///
    /// The one thing SQLite cannot constrain: `result_json` is TEXT, and no `CHECK` can say it is a
    /// [`mixengine_proto::JobOutcome`]. Naming the column is what makes the row findable.
    #[error("job #{job} has a {column} this build cannot read")]
    UnreadableJobRow {
        /// The job whose row cannot be read.
        job: i64,
        /// Which column.
        column: &'static str,
        /// How it failed to parse.
        #[source]
        source: serde_json::Error,
    },

    /// A job's outcome could not be written as JSON.
    ///
    /// Unreachable — a [`mixengine_proto::JobOutcome`] is one of ours, and the only value in it that
    /// this crate did not construct came out of a JSON document itself. Reported rather than
    /// unwrapped because nothing in this crate panics.
    #[error("what job #{job} produced could not be stored")]
    JobOutcomeUnwritable {
        /// The job.
        job: i64,
        /// How it failed to serialise.
        #[source]
        source: serde_json::Error,
    },
}
