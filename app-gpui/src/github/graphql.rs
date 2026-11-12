//! The GraphQL documents of IntelliJ's GitHub plugin, verbatim: its fragments (`resources/graphql/fragment`,
//! plus collaboration-tools' `pageInfo`), each named after its file (the name it is spread with,
//! `...name`), and its queries and mutations (`resources/graphql/query`), named after their files.
//! [`crate::github::api::with_fragments`] adds the fragments a document needs before it is sent.

/// Fragments by name.
pub const FRAGMENTS: [(&str, &str); 39] = [
    (
        "actor",
        r####"
  fragment actor on Actor {
    ...actorInfo

    ... on User {
      ...userInfo
    }
    ... on EnterpriseUserAccount {
      ...enterpriseUserAccountInfo
    }
    ... on Organization {
      ...organizationInfo
    }
  }
"####,
    ),
    (
        "actorInfo",
        r####"
  # Just the data that should be present for any type of Actor.
  fragment actorInfo on Actor {
    __typename
    ...nodeInfo
    login
    url
    avatarUrl
  }
"####,
    ),
    (
        "checkRun",
        r####"
  fragment checkRun on CheckRun {
    name
    conclusion
    isRequired(pullRequestNumber: $number) # See: findPullRequestMergeability
    detailsUrl
    url
  }
"####,
    ),
    (
        "checkSuite",
        r####"
  fragment checkSuite on CheckSuite {
    checkRuns(first: 100) {
      pageInfo {
        ...pageInfo
      }
      nodes {
        ...checkRun
      }
    }
  }
"####,
    ),
    (
        "comment",
        r####"
  fragment comment on Comment {
    ...nodeInfo
    author {
      ...actor
    }
    body
    createdAt
    ...reactions
  }
"####,
    ),
    (
        "commit",
        r####"
  fragment commit on Commit {
      ...commitShort
      committer {
          ...gitActorInfo
      }
      messageBody
      committedDate
      parents(first: 100) {
          nodes {
              ...commitHash
          }
      }
  }
"####,
    ),
    (
        "commitHash",
        r####"
  fragment commitHash on Commit {
      ...nodeInfo
      oid
      abbreviatedOid
  }
"####,
    ),
    (
        "commitShort",
        r####"
  fragment commitShort on Commit {
      ...commitHash
      url
      messageHeadline
      author {
          ...gitActorInfo
      }
  }
"####,
    ),
    (
        "commitStatusContext",
        r####"
  fragment commitStatusContext on StatusContext {
    context
    state
    isRequired(pullRequestNumber: $number) # See: findPullRequestMergeability
    targetUrl
  }
"####,
    ),
    (
        "commitStatusRollup",
        r####"
  fragment commitStatusRollup on StatusCheckRollup {
    contexts(first: $pageSize, after: $cursor) {
      pageInfo {
        ...pageInfo
      }
      nodes {
        __typename
        ... on CheckRun {
          checkSuite {
            workflowRun {
              workflow {
                name
              }
            }
          }
          name
          title
          status
          conclusion
          url
        }
        ... on StatusContext {
          context
          description
          state
          targetUrl
        }
      }
    }
  }
"####,
    ),
    (
        "commitStatusRollupShort",
        r####"
  fragment commitStatusRollupShort on StatusCheckRollup {
    state
  }
"####,
    ),
    (
        "commitWithCheckStatuses",
        r####"
  fragment commitWithCheckStatuses on Commit {
    ...commitHash
    status {
      contexts {
        ...commitStatusContext
      }
    }

    checkSuites(first: 100) {
      pageInfo {
        ...pageInfo
      }
      nodes {
        ...checkSuite
      }
    }
  }
"####,
    ),
    (
        "enterpriseUserAccountInfo",
        r####"
  fragment enterpriseUserAccountInfo on EnterpriseUserAccount {
    name
  }
"####,
    ),
    (
        "gitActorInfo",
        r####"
  fragment gitActorInfo on GitActor {
    name
    email
    avatarUrl
    date
    user {
      ...user
    }
  }
"####,
    ),
    (
        "gitRefName",
        r####"
  fragment gitRefName on Ref {
    name
  }
"####,
    ),
    (
        "labelInfo",
        r####"
  fragment labelInfo on Label {
    ...nodeInfo
    url
    name
    color
  }
"####,
    ),
    (
        "nodeInfo",
        r####"
  fragment nodeInfo on Node {
    id
  }
"####,
    ),
    (
        "organizationInfo",
        r####"
  fragment organizationInfo on Organization {
    name
  }
"####,
    ),
    (
        "pageInfo",
        r####"
  fragment pageInfo on PageInfo {
      startCursor
      hasPreviousPage
      endCursor
      hasNextPage
  }
"####,
    ),
    (
        "pullRequestBranchMatch",
        r####"
  fragment pullRequestBranchMatch on PullRequest {
    ...nodeInfo
    number
    title
    headRefName
  }
"####,
    ),
    (
        "pullRequestCommitWithCheckStatuses",
        r####"
  fragment pullRequestCommitWithCheckStatuses on PullRequestCommit {
    ...nodeInfo
    commit {
      ...commitWithCheckStatuses
    }
  }
"####,
    ),
    (
        "pullRequestInfo",
        r####"
  fragment pullRequestInfo on PullRequest {
    ...pullRequestInfoShort

    body

    author {
      ...actor
    }
    createdAt

    reviewDecision

    baseRefName
    baseRefOid
    baseRepository {
      ...repository
    }

    headRef {
      id
    }
    headRefName
    headRefOid
    headRepository {
      ...repository
    }

    viewerCanDeleteHeadRef
  }
"####,
    ),
    (
        "pullRequestInfoShort",
        r####"
  fragment pullRequestInfoShort on PullRequest {
    ...nodeInfo
    url
    number

    title
    state
    isDraft

    author {
      ...actor
    }
    createdAt
    updatedAt

    isReadByViewer

    assignees(first: 100) {
      nodes {
        ...user
      }
    }
    labels(first: 100) {
      nodes {
        ...labelInfo
      }
    }
    reviewRequests(first: 100) {
      nodes {
        ... on ReviewRequest {
          requestedReviewer {
            ...pullRequestReviewer
          }
        }
      }
    }
    reviewThreads(last: 100) {
      nodes {
        isResolved
        isOutdated
      }
    }
    reviews(last: 100) {
      nodes {
        ...pullRequestReview
      }
    }
    ...reactions

    mergeable

    viewerCanUpdate
    viewerCanReact
    viewerDidAuthor
  }
"####,
    ),
    (
        "pullRequestMergeability",
        r####"
  fragment pullRequestMergeability on PullRequest {
    mergeable
    canBeRebased
    mergeStateStatus
    baseRef {
      refUpdateRule {
        ...refUpdateRule
      }
    }

    commits(last: 1) {
      pageInfo {
        ...pageInfo
      }
      nodes {
        ...pullRequestCommitWithCheckStatuses
      }
    }
  }
"####,
    ),
    (
        "pullRequestPendingReview",
        r####"
  fragment pullRequestPendingReview on PullRequestReview {
    ...nodeInfo
    state
    comments(first: 0) {
      totalCount
    }
  }
"####,
    ),
    (
        "pullRequestReview",
        r####"
  fragment pullRequestReview on PullRequestReview {
    __typename
    ...nodeInfo
    url
    author {
      ...actor
    }
    body
    state
    createdAt
    viewerCanUpdate
  }
"####,
    ),
    (
        "pullRequestReviewComment",
        r####"
  fragment pullRequestReviewComment on PullRequestReviewComment {
    ...comment
    url
    state
    commit {
      ...commitHash
    }
    originalCommit {
      ...commitHash
    }
    diffHunk
    pullRequestReview {
      ...nodeInfo
    }
    viewerCanDelete
    viewerCanUpdate
    viewerCanReact
  }
"####,
    ),
    (
        "pullRequestReviewer",
        r####"
  fragment pullRequestReviewer on RequestedReviewer {
    ... on Actor {
      ...actorInfo
    }
    ... on User {
      ...userInfo
    }
    ... on Team {
      ...teamInfo
    }

    # For future enthusiasts: be careful with adding ... on Bot, as it's been added only in GHE 3.11
  }
"####,
    ),
    (
        "pullRequestReviewThread",
        r####"
  fragment pullRequestReviewThread on PullRequestReviewThread {
    ...nodeInfo
    isResolved
    isOutdated
    path
    subjectType
    diffSide
    line
    originalLine
    startDiffSide
    startLine
    originalStartLine
    comments(first: 100) {
      nodes {
        ...pullRequestReviewComment
      }
    }
    viewerCanReply
    viewerCanResolve
    viewerCanUnresolve
  }
"####,
    ),
    (
        "pullRequestTemplate",
        r####"
  fragment pullRequestTemplate on PullRequestTemplate {
    body
  }
"####,
    ),
    (
        "rates",
        r####"
  fragment rates on RateLimit {
    cost
  }
"####,
    ),
    (
        "reaction",
        r####"
  fragment reaction on Reaction {
    content
    user {
      ...user
    }
  }
"####,
    ),
    (
        "reactions",
        r####"
  fragment reactions on Reactable {
    reactions(first: 25) {
      pageInfo {
        ...pageInfo
      }
      nodes {
        ...reaction
      }
    }
  }
"####,
    ),
    (
        "refUpdateRule",
        r####"
  fragment refUpdateRule on RefUpdateRule {
    allowsDeletions
    allowsForcePushes
    pattern
    requiredApprovingReviewCount
    requiredStatusCheckContexts
    requiresLinearHistory
    requiresSignatures
    viewerCanPush
  }
"####,
    ),
    (
        "repository",
        r####"
  fragment repository on Repository {
    ...nodeInfo
    owner {
      ...repositoryOwnerName
    }
    nameWithOwner
    viewerPermission

    mergeCommitAllowed
    squashMergeAllowed
    rebaseMergeAllowed

    defaultBranchRef {
      ...gitRefName
    }
    isFork
    url
    sshUrl
  }
"####,
    ),
    (
        "repositoryOwnerName",
        r####"
  fragment repositoryOwnerName on RepositoryOwner {
      __typename
      ...nodeInfo
      login
  }
"####,
    ),
    (
        "teamInfo",
        r####"
  fragment teamInfo on Team {
      __typename
      ...nodeInfo
      slug
      url
      teamAvatarUrl: avatarUrl
      teamName: name
      combinedSlug
  }
"####,
    ),
    (
        "user",
        r####"
  fragment user on User {
    ...actorInfo
    ...userInfo
  }
"####,
    ),
    (
        "userInfo",
        r####"
  fragment userInfo on User {
    name
  }
"####,
    ),
];

/// Queries and mutations by name.
pub const QUERIES: [(&str, &str); 40] = [
    (
        "addPullRequestReviewThread",
        r####"
mutation ($body: String!, $line: Int!, $path: String!, $pullRequestReviewId: ID, $side: DiffSide, $startSide: DiffSide, $startLine: Int) {
  addPullRequestReviewThread(input: {body: $body, line: $line, path: $path, pullRequestReviewId:$pullRequestReviewId, side: $side, startSide: $startSide, startLine: $startLine}) {
    thread {
      ...pullRequestReviewThread
    }
  }
}
"####,
    ),
    (
        "addReaction",
        r####"
mutation ($id: ID!, $reaction: ReactionContent!) {
  addReaction(input: {
    subjectId: $id,
    content: $reaction
  }) {
    reaction {
      ...reaction
    }
  }
}
"####,
    ),
    (
        "addReviewComment",
        r####"
mutation ($reviewId: ID!, $body: String!, $commit: GitObjectID, $file: String, $position: Int, $inReplyTo: ID) {
  addPullRequestReviewComment(input: {pullRequestReviewId: $reviewId, body: $body, commitOID: $commit, path: $file, position: $position, inReplyTo: $inReplyTo}) {
    comment {
      ...pullRequestReviewComment
    }
  }
}
"####,
    ),
    (
        "createPullRequest",
        r####"
mutation ($repositoryId: ID!, $baseRefName: String!, $headRefName: String!, $title: String!, $body: String, $draft: Boolean = false) {
  createPullRequest(input: {
    repositoryId: $repositoryId,
    baseRefName: $baseRefName,
    headRefName: $headRefName,
    title: $title,
    body: $body,
    draft: $draft
  }) {
    pullRequest {
      ...pullRequestInfoShort
    }
  }
}
"####,
    ),
    (
        "createReview",
        r####"
mutation ($pullRequestId: ID!, $event: PullRequestReviewEvent, $body: String, $commitOid: GitObjectID, $threads: [DraftPullRequestReviewThread]) {
  addPullRequestReview(input: {pullRequestId: $pullRequestId, event: $event, body: $body, commitOID: $commitOid, threads: $threads}) {
    pullRequestReview {
      ...pullRequestPendingReview
    }
  }
}
"####,
    ),
    (
        "deleteIssueComment",
        r####"
mutation ($id: ID!) {
    deleteIssueComment(input: {id: $id}) {
        __typename
    }
}
"####,
    ),
    (
        "deleteRef",
        r####"
mutation ($refId: ID!) {
  deleteRef(input: {refId: $refId}) {
    clientMutationId
  }
}
"####,
    ),
    (
        "deleteReview",
        r####"
mutation ($reviewId: ID!){
    deletePullRequestReview(input: {pullRequestReviewId: $reviewId}) {
        __typename
    }
}
"####,
    ),
    (
        "deleteReviewComment",
        r####"
mutation ($id: ID!) {
    deletePullRequestReviewComment(input: {id: $id}) {
        pullRequestReview {
            ...pullRequestPendingReview
        }
    }
}
"####,
    ),
    (
        "findMentionableUsers",
        r####"
query($repoOwner: String!, $repoName: String!, $pageSize: Int = 100, $cursor: String) {
  repository(owner: $repoOwner, name: $repoName) {
    mentionableUsers(first: $pageSize, after: $cursor) {
      pageInfo {
        ...pageInfo
      }
      nodes {
        ...user
      }
    }
  }

  rateLimit {
    ...rates
  }
}
"####,
    ),
    (
        "findOrganizationTeams",
        r####"
query($organization: String!, $pageSize: Int = 100, $cursor: String) {
  organization(login: $organization) {
    teams(first: $pageSize, after: $cursor) {
      pageInfo {
        ...pageInfo
      }
      nodes {
        ...teamInfo
      }
    }
  }

  rateLimit {
    ...rates
  }
}
"####,
    ),
    (
        "findPendingReview",
        r####"
query($pullRequestId: ID!) {
  node(id: $pullRequestId) {
    ... on PullRequest {
      reviews(states: [PENDING], first: 1) {
        nodes {
          ...pullRequestPendingReview
        }
      }
    }
  }

  rateLimit {
    ...rates
  }
}
"####,
    ),
    (
        "findPullRequest",
        r####"
query($repoOwner: String!, $repoName: String!, $number: Int!) {
  repository(owner: $repoOwner, name: $repoName) {
    pullRequest(number: $number) {
      ...pullRequestInfo
    }
  }

  rateLimit {
    ...rates
  }
}
"####,
    ),
    (
        "findPullRequestId",
        r####"
query($repoOwner: String!, $repoName: String!, $number: Int!) {
  repository(owner: $repoOwner, name: $repoName) {
    pullRequest(number: $number) {
      ...nodeInfo
      number
    }
  }

  rateLimit {
    ...rates
  }
}
"####,
    ),
    (
        "findPullRequestMergeability",
        r####"
query($repoOwner: String!, $repoName: String!, $number: Int!) {
  repository(owner: $repoOwner, name: $repoName) {
    pullRequest(number: $number) {
      ...pullRequestMergeability
    }
  }

  rateLimit {
    ...rates
  }
}
"####,
    ),
    (
        "findPullRequestsByHeadBranch",
        r####"
query findPullRequestsByHeadBranch($query: String!, $pageSize: Int = 100, $cursor: String) {
  search(first: $pageSize, after: $cursor, type: ISSUE_ADVANCED, query: $query) {
    pageInfo {
      ...pageInfo
    }
    nodes {
      ...pullRequestBranchMatch
    }
  }

  rateLimit {
    ...rates
  }
}
"####,
    ),
    (
        "findRepository",
        r####"
query($repoOwner: String!, $repoName: String!) {
  repository(owner: $repoOwner, name: $repoName) {
    ...repository
  }

  rateLimit {
    ...rates
  }
}
"####,
    ),
    (
        "findUser",
        r####"
query($login: String!) {
  user(login: $login) {
    __typename
    ...user
  }

  rateLimit {
    ...rates
  }
}
"####,
    ),
    (
        "getProtectionRules",
        r####"
query($repoOwner: String!, $repoName: String!, $pageSize: Int = 100, $cursor: String) {
  repository(owner: $repoOwner, name: $repoName) {
    branchProtectionRules(first: $pageSize, after: $cursor) {
      pageInfo {
        ...pageInfo
      }
      nodes {
        ...nodeInfo
        pattern
      }
    }
  }

  rateLimit {
    ...rates
  }
}
"####,
    ),
    (
        "getPullRequestTemplates",
        r####"
query($repoOwner: String!, $repoName: String!) {
  repository(owner: $repoOwner, name: $repoName) {
    pullRequestTemplates {
      ...pullRequestTemplate
    }
  }

  rateLimit {
    ...rates
  }
}
"####,
    ),
    (
        "getRepositoryCommitStatusContexts",
        r####"
query($repoOwner: String!, $repoName: String!, $oid: GitObjectID!, $pageSize: Int, $cursor: String) {
  repository(owner: $repoOwner, name: $repoName) {
    object(oid: $oid) {
      ... on Commit {
        statusCheckRollup {
          ...commitStatusRollup
        }
      }
    }
  }

  rateLimit {
    ...rates
  }
}
"####,
    ),
    (
        "getRepositoryCommitStatusShort",
        r####"
query($repoOwner: String!, $repoName: String!, $oid: GitObjectID!) {
  repository(owner: $repoOwner, name: $repoName) {
    object(oid: $oid) {
      ... on Commit {
        statusCheckRollup {
          ...commitStatusRollupShort
        }
      }
    }
  }

  rateLimit {
    ...rates
  }
}
"####,
    ),
    (
        "issueSearch",
        r####"
query issueSearch($query: String!, $pageSize: Int = 100, $cursor: String) {
  search(first: $pageSize, after: $cursor, type: ISSUE_ADVANCED, query: $query) {
    pageInfo {
      ...pageInfo
    }
    nodes {
      ...pullRequestInfoShort
    }
  }

  rateLimit {
    ...rates
  }
}
"####,
    ),
    (
        "markFileAsViewed",
        r####"
mutation ($pullRequestId: ID!, $path: String!) {
    markFileAsViewed(input: { pullRequestId: $pullRequestId, path: $path }) {
        __typename
    }
}
"####,
    ),
    (
        "markPullRequestReadyForReview",
        r####"
mutation ($pullRequestId: ID!) {
    markPullRequestReadyForReview(input: {pullRequestId: $pullRequestId}) {
        __typename
    }
}
"####,
    ),
    (
        "metrics",
        r####"
query metrics(
  $allPRCountQuery: String!,
  $openPRCountQuery: String!,
  $openAuthoredPRCountQuery: String!,
  $openAssigneePRCountQuery: String!,
  $openReviewedPRCountQuery: String!,
  $openReviewAssignedPRCountQuery: String!,
) {
  allPRCount: search(type: ISSUE, query: $allPRCountQuery) {
    issueCount
  }

  openPRCount: search(type: ISSUE, query: $openPRCountQuery) {
    issueCount
  }

  openAuthoredPRCount: search(type: ISSUE, query: $openAuthoredPRCountQuery) {
    issueCount
  }

  openAssigneePRCount: search(type: ISSUE, query: $openAssigneePRCountQuery) {
    issueCount
  }

  openReviewedPRCount: search(type: ISSUE, query: $openReviewedPRCountQuery) {
    issueCount
  }

  openReviewAssignedPRCount: search(type: ISSUE, query: $openReviewAssignedPRCountQuery) {
    issueCount
  }

  rateLimit {
    ...rates
  }
}
"####,
    ),
    (
        "pullRequestCommits",
        r####"
query($repoOwner: String!, $repoName: String!, $number: Int!, $pageSize: Int = 100, $cursor: String) {
  repository(owner: $repoOwner, name: $repoName) {
    pullRequest(number: $number) {
      commits(first: $pageSize, after: $cursor) {
        pageInfo {
          ...pageInfo
        }
        nodes {
          ...nodeInfo
          commit {
            ...commit
          }
          url
        }
      }
    }
  }

  rateLimit {
    ...rates
  }
}
"####,
    ),
    (
        "pullRequestFiles",
        r####"
query($repoOwner: String!, $repoName: String!, $number: Int!, $pageSize: Int = 100, $cursor: String) {
  repository(owner: $repoOwner, name: $repoName) {
    pullRequest(number: $number) {
      files(first: $pageSize, after: $cursor) {
        pageInfo {
          ...pageInfo
        }
        nodes {
          path
          viewerViewedState
        }
      }
    }
  }

  rateLimit {
    ...rates
  }
}
"####,
    ),
    (
        "pullRequestParticipants",
        r####"
query($repoOwner: String!, $repoName: String!, $number: Int!, $pageSize: Int = 100, $cursor: String) {
  repository(owner: $repoOwner, name: $repoName) {
    pullRequest(number: $number) {
      participants(first: $pageSize, after: $cursor) {
        pageInfo {
          ...pageInfo
        }
        nodes {
          ...user
        }
      }
    }
  }

  rateLimit {
    ...rates
  }
}
"####,
    ),
    (
        "pullRequestReviewThreads",
        r####"
query($repoOwner: String!, $repoName: String!, $number: Int!, $pageSize: Int = 100, $cursor: String) {
  repository(owner: $repoOwner, name: $repoName) {
    pullRequest(number: $number) {
      reviewThreads(first: $pageSize, after: $cursor) {
        pageInfo {
          ...pageInfo
        }
        nodes {
          ...pullRequestReviewThread
        }
      }
    }
  }

  rateLimit {
    ...rates
  }
}
"####,
    ),
    (
        "pullRequestTimeline",
        r####"
fragment issueCommentInfo on IssueComment {
  ...comment
  viewerCanUpdate
  viewerCanDelete
  viewerCanReact
}

fragment prCommitInfo on PullRequestCommit {
  ...nodeInfo
  commit {
    ...commitShort
  }
  url
}

fragment reviewDismissedEvent on ReviewDismissedEvent {
  actor {
    ...actor
  }
  createdAt
  dismissalMessageHTML
  review {
    author {
      ...actor
    }
  }
}

fragment readyForReviewEvent on ReadyForReviewEvent {
  actor {
    ...actor
  }
  createdAt
}

fragment convertToDraftEvent on ConvertToDraftEvent {
  actor {
    ...actor
  }
  createdAt
}

fragment renamedEvent on RenamedTitleEvent {
  actor {
    ...actor
  }
  previousTitle
  currentTitle
  createdAt
}

fragment closedEvent on ClosedEvent {
  actor {
    ...actor
  }
  createdAt
}

fragment reopenedEvent on ReopenedEvent {
  actor {
    ...actor
  }
  createdAt
}

fragment mergedEvent on MergedEvent {
  actor {
    ...actor
  }
  createdAt
  mergedEventCommit: commit {
    ...commitShort
  }
  mergeRefName
}

fragment assignedEvent on AssignedEvent {
  actor {
    ...actor
  }
  createdAt
  user {
    ...user
  }
}

fragment unassignedEvent on UnassignedEvent {
  actor {
    ...actor
  }
  createdAt
  user {
    ...user
  }
}

fragment labeledEvent on LabeledEvent {
  actor {
    ...actor
  }
  createdAt
  label {
    ...labelInfo
  }
}

fragment unlabeledEvent on UnlabeledEvent {
  actor {
    ...actor
  }
  createdAt
  label {
    ...labelInfo
  }
}

fragment reviewRequestedEvent on ReviewRequestedEvent {
  actor {
    ...actor
  }
  createdAt
  requestedReviewer {
    ...pullRequestReviewer
  }
}

fragment reviewRequestRemovedEvent on ReviewRequestRemovedEvent {
  actor {
    ...actor
  }
  createdAt
  requestedReviewer {
    ...pullRequestReviewer
  }
}

fragment gitRefName on Ref {
  name
}

fragment baseRefForcePushedEvent on BaseRefForcePushedEvent {
  actor {
    ...actor
  }
  createdAt
  ref {
    ...gitRefName
  }
}

fragment headRefForcePushedEvent on HeadRefForcePushedEvent {
  actor {
    ...actor
  }
  createdAt
  ref {
    ...gitRefName
  }
  beforeCommit {
    ...commitHash
  }
  afterCommit {
    ...commitHash
  }
}

fragment headRefDeletedEvent on HeadRefDeletedEvent {
  actor {
    ...actor
  }
  createdAt
  headRefName
}

fragment headRefRestoredEvent on HeadRefRestoredEvent {
  actor {
    ...actor
  }
  createdAt
}

fragment baseRefChangedEvent on BaseRefChangedEvent {
  actor {
    ...actor
  }
  createdAt
}

fragment referenceSubject on ReferencedSubject {
  __typename
  ... on PullRequest {
    title
    number
    prState: state
    isDraft
    url
  }
  ... on Issue {
    title
    number
    issueState: state
    url
  }
}

fragment crossReferencedEvent on CrossReferencedEvent {
  actor {
    ...actor
  }
  createdAt
  source {
    ...referenceSubject
  }
}

fragment connectedEvent on ConnectedEvent {
  actor {
    ...actor
  }
  createdAt
  subject {
    ...referenceSubject
  }
}

fragment disconnectedEvent on DisconnectedEvent {
  actor {
    ...actor
  }
  createdAt
  subject {
    ...referenceSubject
  }
}

query($repoOwner: String!, $repoName: String!, $number: Int!, $pageSize: Int = 100, $cursor: String, $since: DateTime) {
  repository(owner: $repoOwner, name: $repoName) {
    pullRequest(number: $number) {
      timelineItems(first: $pageSize, after: $cursor, since: $since) {
        pageInfo {
          ...pageInfo
        }
        nodes {
          __typename
          ...prCommitInfo
          ...pullRequestReview
          ...issueCommentInfo

          ...renamedEvent

          ...closedEvent
          ...reopenedEvent
          ...mergedEvent

          ...assignedEvent
          ...unassignedEvent

          ...labeledEvent
          ...unlabeledEvent

          ...reviewRequestedEvent
          ...reviewRequestRemovedEvent

          ...reviewDismissedEvent

          ...readyForReviewEvent
          ...convertToDraftEvent

          ...baseRefChangedEvent
          ...baseRefForcePushedEvent

          ...headRefForcePushedEvent
          ...headRefDeletedEvent
          ...headRefRestoredEvent

          ...crossReferencedEvent
          ...connectedEvent
          ...disconnectedEvent
        }
      }
    }
  }

  rateLimit {
    ...rates
  }
}
"####,
    ),
    (
        "removeReaction",
        r####"
mutation ($id: ID!, $reaction: ReactionContent!) {
  removeReaction(input: {
    subjectId: $id,
    content: $reaction
  }) {
    reaction {
      ...reaction
    }
  }
}
"####,
    ),
    (
        "resolveReviewThread",
        r####"
mutation ($threadId: ID!) {
    resolveReviewThread(input: {threadId: $threadId}) {
        thread {
            ...pullRequestReviewThread
        }
    }
}
"####,
    ),
    (
        "submitReview",
        r####"
mutation ($reviewId: ID!, $event: PullRequestReviewEvent!, $body: String){
    submitPullRequestReview(input: {pullRequestReviewId: $reviewId, event: $event, body: $body}) {
        __typename
    }
}
"####,
    ),
    (
        "unmarkFileAsViewed",
        r####"
mutation ($pullRequestId: ID!, $path: String!) {
    unmarkFileAsViewed(input: { pullRequestId: $pullRequestId, path: $path }) {
        __typename
    }
}
"####,
    ),
    (
        "unresolveReviewThread",
        r####"
mutation ($threadId: ID!) {
    unresolveReviewThread(input: {threadId: $threadId}) {
        thread {
            ...pullRequestReviewThread
        }
    }
}
"####,
    ),
    (
        "updateIssueComment",
        r####"
mutation ($id: ID!, $body: String!) {
    updateIssueComment(input: {id: $id, body: $body}) {
        issueComment {
            ...comment
        }
    }
}
"####,
    ),
    (
        "updatePullRequest",
        r####"
mutation ($pullRequestId: ID!, $title: String, $body: String) {
    updatePullRequest(input: {pullRequestId: $pullRequestId, title: $title, body: $body}) {
        pullRequest {
            ...pullRequestInfo
        }
    }
}
"####,
    ),
    (
        "updateReview",
        r####"
mutation ($reviewId: ID!, $body: String!){
    updatePullRequestReview(input: {pullRequestReviewId: $reviewId, body: $body}) {
        pullRequestReview {
            ...pullRequestReview
        }
    }
}
"####,
    ),
    (
        "updateReviewComment",
        r####"
mutation ($id: ID!, $body: String!) {
    updatePullRequestReviewComment(input: {pullRequestReviewCommentId: $id, body: $body}) {
        pullRequestReviewComment {
            ...pullRequestReviewComment
        }
    }
}
"####,
    ),
];

/// The document named `name` (a query or mutation of [`QUERIES`]).
pub fn query(name: &str) -> Option<&'static str> {
    QUERIES.iter().find(|(n, _)| *n == name).map(|(_, text)| *text)
}

/// The fragment named `name`.
pub fn fragment(name: &str) -> Option<&'static str> {
    FRAGMENTS.iter().find(|(n, _)| *n == name).map(|(_, text)| *text)
}
