mod array;
mod attrs;
mod change;
mod delta;
mod doc;
mod error;
mod map;
mod mapchange;
mod text;
mod transaction;
mod undo;
mod xml;
mod subscription;

use crate::doc::YrsCollectionPtr;
use crate::doc::YrsOrigin;
use crate::array::YrsArray;
use crate::array::YrsArrayEachDelegate;
use crate::array::YrsArrayObservationDelegate;
use crate::change::YrsChange;
use crate::delta::YrsDelta;
use crate::doc::YrsDoc;
use crate::error::CodingError;
use crate::map::YrsMap;
use crate::map::YrsMapIteratorDelegate;
use crate::map::YrsMapKVIteratorDelegate;
use crate::map::YrsMapObservationDelegate;
use crate::mapchange::YrsEntryChange;
use crate::mapchange::YrsMapChange;
use crate::text::YrsText;
use crate::text::YrsTextObservationDelegate;
use crate::transaction::YrsTransaction;
use crate::undo::YrsUndoManager;
use crate::undo::YrsUndoError;
use crate::undo::YrsUndoManagerObservationDelegate;
use crate::undo::YrsUndoEvent;
use crate::undo::YrsUndoEventKind;
use crate::xml::{YrsXmlAttribute, YrsXmlNode, YrsXmlResolvedPosition, YrsXmlTextRun};

fn merge_updates_v1(updates: Vec<Vec<u8>>) -> Result<Vec<u8>, CodingError> {
    yrs::merge_updates_v1(updates.iter().map(|update| update.as_slice()))
        .map_err(|_| CodingError::DecodingError)
}
use crate::subscription::YSubscription;

uniffi::include_scaffolding!("yniffi");
