// Copyright 2019 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{rpc::Rpc, vault::Init, Result};
use bincode;
use log::{error, trace};
use pickledb::{PickleDb, PickleDbDumpPolicy};
use rand::{distributions::Standard, thread_rng, Rng};
use safe_nd::{
    ADataAddress, ClientPublicId, Error as NdError, IDataAddress, PublicId, PublicKey, Request,
    Response, XorName,
};
use serde::Serialize;
use std::{borrow::Cow, fs, path::Path};
use unwrap::unwrap;

pub(crate) fn new_db<D: AsRef<Path>, N: AsRef<Path>>(
    db_dir: D,
    db_name: N,
    init_mode: Init,
) -> Result<PickleDb> {
    let db_path = db_dir.as_ref().join(db_name);
    if init_mode == Init::New {
        trace!("Creating database at {}", db_path.display());
        fs::create_dir_all(db_dir)?;
        let mut db = PickleDb::new_bin(db_path, PickleDbDumpPolicy::AutoDump);
        // Write then delete a value to ensure DB file is actually written to disk.
        db.set("", &"")?;
        let _ = db.rem("")?;
        return Ok(db);
    }
    trace!("Loading database at {}", db_path.display());
    let result = PickleDb::load_bin(db_path.clone(), PickleDbDumpPolicy::AutoDump);
    if let Err(ref error) = &result {
        error!("Failed to load {}: {}", db_path.display(), error);
    }
    Ok(result?)
}

pub(crate) fn random_vec(size: usize) -> Vec<u8> {
    thread_rng().sample_iter(&Standard).take(size).collect()
}

pub(crate) fn serialise<T: Serialize>(data: &T) -> Vec<u8> {
    unwrap!(bincode::serialize(data))
}

/// Returns the client's public ID, the owner's public ID, or None depending on whether `public_id`
/// represents a Client, App or Node respectively.
pub(crate) fn owner(public_id: &PublicId) -> Option<&ClientPublicId> {
    match public_id {
        PublicId::Node(_) => None,
        PublicId::Client(pub_id) => Some(pub_id),
        PublicId::App(pub_id) => Some(pub_id.owner()),
    }
}

/// Returns the client's ID if `public_id` represents a Client, or None if it represents an App or
/// Node.
pub(crate) fn client(public_id: &PublicId) -> Option<&ClientPublicId> {
    match public_id {
        PublicId::Node(_) | PublicId::App(_) => None,
        PublicId::Client(pub_id) => Some(pub_id),
    }
}

/// Returns the client's or app's public key if `public_id` represents a Client or App respectively,
/// or None if it represents a Node.
pub(crate) fn own_key(public_id: &PublicId) -> Option<&PublicKey> {
    match public_id {
        PublicId::Node(_) => None,
        PublicId::Client(ref client) => Some(client.public_key()),
        PublicId::App(ref app) => Some(app.public_key()),
    }
}

/// Returns the requester's address.  An App's address is the name of its owner.
pub(crate) fn requester_address(rpc: &Rpc) -> &XorName {
    match rpc {
        Rpc::Request { ref requester, .. } | Rpc::Response { ref requester, .. } => {
            requester.name()
        }
    }
}

/// Returns the address of the destination for `request`.
pub(crate) fn destination_address(request: &Request) -> Option<Cow<XorName>> {
    use Request::*;
    match request {
        PutIData(ref data) => Some(Cow::Borrowed(data.name())),
        GetIData(ref address) => Some(Cow::Borrowed(address.name())),
        DeleteUnpubIData(ref address) => Some(Cow::Borrowed(address.name())),
        PutMData(ref data) => Some(Cow::Borrowed(data.name())),
        GetMData(ref address)
        | GetMDataValue { ref address, .. }
        | DeleteMData(ref address)
        | GetMDataShell(ref address)
        | GetMDataVersion(ref address)
        | ListMDataEntries(ref address)
        | ListMDataKeys(ref address)
        | ListMDataValues(ref address)
        | SetMDataUserPermissions { ref address, .. }
        | DelMDataUserPermissions { ref address, .. }
        | ListMDataPermissions(ref address)
        | ListMDataUserPermissions { ref address, .. }
        | MutateSeqMDataEntries { ref address, .. }
        | MutateUnseqMDataEntries { ref address, .. } => Some(Cow::Borrowed(address.name())),
        PutAData(ref data) => Some(Cow::Borrowed(data.name())),
        GetAData(ref address)
        | GetADataValue { ref address, .. }
        | GetADataShell { ref address, .. }
        | DeleteAData(ref address)
        | GetADataRange { ref address, .. }
        | GetADataIndices(ref address)
        | GetADataLastEntry(ref address)
        | GetADataPermissions { ref address, .. }
        | GetPubADataUserPermissions { ref address, .. }
        | GetUnpubADataUserPermissions { ref address, .. }
        | GetADataOwners { ref address, .. }
        | AddPubADataPermissions { ref address, .. }
        | AddUnpubADataPermissions { ref address, .. }
        | SetADataOwner { ref address, .. } => Some(Cow::Borrowed(address.name())),
        AppendSeq { ref append, .. } | AppendUnseq(ref append) => {
            Some(Cow::Borrowed(append.address.name()))
        }
        TransferCoins {
            ref destination, ..
        } => Some(Cow::Borrowed(destination)),
        CreateBalance {
            ref new_balance_owner,
            ..
        } => Some(Cow::Owned(XorName::from(*new_balance_owner))),
        CreateLoginPacket(login_packet) => Some(Cow::Borrowed(login_packet.destination())),
        CreateLoginPacketFor {
            new_login_packet, ..
        } => Some(Cow::Borrowed(new_login_packet.destination())),
        UpdateLoginPacket(login_packet) => Some(Cow::Borrowed(login_packet.destination())),
        GetLoginPacket(ref name) => Some(Cow::Borrowed(name)),
        GetBalance | ListAuthKeysAndVersion | InsAuthKey { .. } | DelAuthKey { .. } => None,
    }
}

/// Create an error response for the given request.
pub fn to_error_response(request: &Request, error: NdError) -> Response {
    match request {
        Request::PutIData(_)
        | Request::DeleteUnpubIData(_)
        | Request::PutMData(_)
        | Request::DeleteMData(_)
        | Request::SetMDataUserPermissions { .. }
        | Request::DelMDataUserPermissions { .. }
        | Request::MutateSeqMDataEntries { .. }
        | Request::MutateUnseqMDataEntries { .. }
        | Request::PutAData(_)
        | Request::DeleteAData(_)
        | Request::AddPubADataPermissions { .. }
        | Request::AddUnpubADataPermissions { .. }
        | Request::SetADataOwner { .. }
        | Request::AppendSeq { .. }
        | Request::AppendUnseq { .. }
        | Request::CreateLoginPacket(_)
        | Request::CreateLoginPacketFor { .. }
        | Request::UpdateLoginPacket { .. }
        | Request::InsAuthKey { .. }
        | Request::DelAuthKey { .. } => Response::Mutation(Err(error)),
        Request::GetIData(_) => Response::GetIData(Err(error)),
        Request::GetMData(_) => Response::GetMData(Err(error)),
        Request::GetMDataValue { address, .. } => {
            if address.is_seq() {
                Response::GetSeqMDataValue(Err(error))
            } else {
                Response::GetUnseqMDataValue(Err(error))
            }
        }
        Request::GetMDataShell(_) => Response::GetMDataShell(Err(error)),
        Request::GetMDataVersion(_) => Response::GetMDataVersion(Err(error)),
        Request::ListMDataKeys(_) => Response::ListMDataKeys(Err(error)),
        Request::ListMDataValues(address) => {
            if address.is_seq() {
                Response::ListSeqMDataValues(Err(error))
            } else {
                Response::ListUnseqMDataValues(Err(error))
            }
        }
        Request::ListMDataEntries(address) => {
            if address.is_seq() {
                Response::ListSeqMDataEntries(Err(error))
            } else {
                Response::ListUnseqMDataEntries(Err(error))
            }
        }
        Request::ListMDataPermissions(_) => Response::ListMDataPermissions(Err(error)),
        Request::ListMDataUserPermissions { .. } => Response::ListMDataUserPermissions(Err(error)),
        Request::GetAData(_) => Response::GetAData(Err(error)),
        Request::GetADataValue { .. } => Response::GetADataValue(Err(error)),
        Request::GetADataShell { .. } => Response::GetADataShell(Err(error)),
        Request::GetADataRange { .. } => Response::GetADataRange(Err(error)),
        Request::GetADataIndices { .. } => Response::GetADataIndices(Err(error)),
        Request::GetADataLastEntry { .. } => Response::GetADataLastEntry(Err(error)),
        Request::GetADataOwners { .. } => Response::GetADataOwners(Err(error)),
        Request::GetADataPermissions { address, .. } => {
            if adata::is_published(address) {
                Response::GetPubADataPermissionAtIndex(Err(error))
            } else {
                Response::GetUnpubADataPermissionAtIndex(Err(error))
            }
        }
        Request::GetPubADataUserPermissions { .. } => {
            Response::GetPubADataUserPermissions(Err(error))
        }
        Request::GetUnpubADataUserPermissions { .. } => {
            Response::GetUnpubADataUserPermissions(Err(error))
        }
        Request::GetBalance => Response::GetBalance(Err(error)),
        Request::TransferCoins { .. } | Request::CreateBalance { .. } => {
            Response::Transaction(Err(error))
        }
        Request::GetLoginPacket(_) => Response::GetLoginPacket(Err(error)),
        Request::ListAuthKeysAndVersion => Response::ListAuthKeysAndVersion(Err(error)),
    }
}

// The kind of authorisation needed for a reequest.
pub(crate) enum AuthorisationKind {
    // Get request against published data.
    GetPub,
    // Get request against unpublished data.
    GetUnpub,
    // Mutation request.
    Mut,
}

// Returns the type of authorisation needed for the given request.
pub(crate) fn authorisation_kind(request: &Request) -> AuthorisationKind {
    use AuthorisationKind::*;
    use Request::*;

    match request {
        PutIData(_)
        | DeleteUnpubIData(_)
        | PutMData(_)
        | DeleteMData(_)
        | SetMDataUserPermissions { .. }
        | DelMDataUserPermissions { .. }
        | MutateSeqMDataEntries { .. }
        | MutateUnseqMDataEntries { .. }
        | PutAData(_)
        | DeleteAData(_)
        | AddPubADataPermissions { .. }
        | AddUnpubADataPermissions { .. }
        | SetADataOwner { .. }
        | AppendSeq { .. }
        | AppendUnseq(_)
        | TransferCoins { .. }
        | CreateBalance { .. }
        | CreateLoginPacket(_)
        | CreateLoginPacketFor { .. }
        | UpdateLoginPacket(_)
        | InsAuthKey { .. }
        | DelAuthKey { .. } => Mut,
        GetIData(IDataAddress::Pub(_)) => GetPub,
        GetIData(IDataAddress::Unpub(_))
        | GetMData(_)
        | GetMDataValue { .. }
        | GetMDataShell(_)
        | GetMDataVersion(_)
        | ListMDataEntries(_)
        | ListMDataKeys(_)
        | ListMDataValues(_)
        | ListMDataPermissions(_)
        | ListMDataUserPermissions { .. }
        | GetLoginPacket(_)
        | GetBalance
        | ListAuthKeysAndVersion => GetUnpub,
        GetAData(address)
        | GetADataValue { address, .. }
        | GetADataShell { address, .. }
        | GetADataRange { address, .. }
        | GetADataIndices(address)
        | GetADataLastEntry(address)
        | GetADataPermissions { address, .. }
        | GetPubADataUserPermissions { address, .. }
        | GetUnpubADataUserPermissions { address, .. }
        | GetADataOwners { address, .. } => {
            if adata::is_published(address) {
                GetPub
            } else {
                GetUnpub
            }
        }
    }
}

pub(crate) mod adata {
    use super::*;

    pub fn is_published(address: &ADataAddress) -> bool {
        use ADataAddress::*;
        match address {
            PubSeq { .. } | PubUnseq { .. } => true,
            UnpubSeq { .. } | UnpubUnseq { .. } => false,
        }
    }

    pub fn is_sequential(address: &ADataAddress) -> bool {
        use ADataAddress::*;
        match address {
            PubSeq { .. } | UnpubSeq { .. } => true,
            PubUnseq { .. } | UnpubUnseq { .. } => false,
        }
    }
}
