use std::path::Path;
use std::sync::Arc;

use arrow_array::{
    Array, FixedSizeListArray, Float32Array, Int32Array, RecordBatch, RecordBatchIterator,
    StringArray, TimestampMicrosecondArray,
};
use arrow_schema::{DataType, Field, Schema, TimeUnit};
use chrono::{TimeZone, Utc};
use futures::TryStreamExt;
use lancedb::Table;
use lancedb::connection::Connection;
use lancedb::index::Index;
use lancedb::index::vector::IvfPqIndexBuilder;
use lancedb::query::{ExecutableQuery, QueryBase};
use uuid::Uuid;

use crate::error::{MnemoError, Result};
use crate::memory::tombstone::{EvictionReason, Tombstone};
use crate::memory::types::{CompressionLevel, Memory, MemorySource, MemoryType, StorageTier};
use crate::storage::filter::MemoryFilter;

const EMBEDDING_DIMENSIONS: i32 = 384;
const MEMORIES_TABLE: &str = "memories";
const TOMBSTONES_TABLE: &str = "tombstones";

pub struct LanceStore {
    connection: Connection,
    memories_table: Option<Table>,
    tombstones_table: Option<Table>,
}

impl LanceStore {
    pub async fn connect(path: &Path) -> Result<Self> {
        let uri = path
            .to_str()
            .ok_or_else(|| MnemoError::Storage("Invalid path encoding".to_string()))?;

        let connection = lancedb::connect(uri)
            .execute()
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to connect to LanceDB: {e}")))?;

        Ok(Self {
            connection,
            memories_table: None,
            tombstones_table: None,
        })
    }

    fn memories_schema() -> Arc<Schema> {
        Arc::new(Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("content", DataType::Utf8, false),
            Field::new(
                "embedding",
                DataType::FixedSizeList(
                    Arc::new(Field::new("item", DataType::Float32, true)),
                    EMBEDDING_DIMENSIONS,
                ),
                false,
            ),
            Field::new("memory_type", DataType::Utf8, false),
            Field::new("weight", DataType::Float32, false),
            Field::new(
                "created_at",
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new(
                "last_accessed",
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new("access_count", DataType::Int32, false),
            Field::new("conversation_id", DataType::Utf8, true),
            Field::new("source", DataType::Utf8, false),
            Field::new("tier", DataType::Utf8, false),
            Field::new("compression", DataType::Utf8, false),
            Field::new("entities", DataType::Utf8, false),
        ]))
    }

    fn tombstones_schema() -> Arc<Schema> {
        Arc::new(Schema::new(vec![
            Field::new("original_id", DataType::Utf8, false),
            Field::new(
                "evicted_at",
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new("topics", DataType::Utf8, false),
            Field::new("participants", DataType::Utf8, false),
            Field::new(
                "approximate_date",
                DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
                false,
            ),
            Field::new("reason", DataType::Utf8, false),
            Field::new("reason_details", DataType::Utf8, true),
        ]))
    }

    fn create_empty_batch(schema: Arc<Schema>) -> RecordBatch {
        let empty_strings: Vec<Option<&str>> = vec![];
        let empty_floats: Vec<f32> = vec![];
        let empty_timestamps: Vec<i64> = vec![];
        let empty_ints: Vec<i32> = vec![];
        let empty_embeddings: Vec<Option<Vec<Option<f32>>>> = vec![];

        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(empty_strings.clone())),
                Arc::new(StringArray::from(empty_strings.clone())),
                Arc::new(FixedSizeListArray::from_iter_primitive::<
                    arrow_array::types::Float32Type,
                    _,
                    _,
                >(empty_embeddings, EMBEDDING_DIMENSIONS)),
                Arc::new(StringArray::from(empty_strings.clone())),
                Arc::new(Float32Array::from(empty_floats)),
                Arc::new(
                    TimestampMicrosecondArray::from(empty_timestamps.clone()).with_timezone("UTC"),
                ),
                Arc::new(TimestampMicrosecondArray::from(empty_timestamps).with_timezone("UTC")),
                Arc::new(Int32Array::from(empty_ints)),
                Arc::new(StringArray::from(empty_strings.clone())),
                Arc::new(StringArray::from(empty_strings.clone())),
                Arc::new(StringArray::from(empty_strings.clone())),
                Arc::new(StringArray::from(empty_strings.clone())),
                Arc::new(StringArray::from(empty_strings)),
            ],
        )
        .expect("Schema matches columns")
    }

    pub async fn create_memories_table(&mut self) -> Result<()> {
        let schema = Self::memories_schema();
        let batch = Self::create_empty_batch(schema.clone());
        let batches = RecordBatchIterator::new(vec![Ok(batch)], schema);

        let table = self
            .connection
            .create_table(MEMORIES_TABLE, Box::new(batches))
            .execute()
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to create memories table: {e}")))?;

        self.memories_table = Some(table);
        Ok(())
    }

    pub async fn create_tombstones_table(&mut self) -> Result<()> {
        let schema = Self::tombstones_schema();
        let batch = Self::create_empty_tombstones_batch(schema.clone());
        let batches = RecordBatchIterator::new(vec![Ok(batch)], schema);

        let table = self
            .connection
            .create_table(TOMBSTONES_TABLE, Box::new(batches))
            .execute()
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to create tombstones table: {e}")))?;

        self.tombstones_table = Some(table);
        Ok(())
    }

    fn create_empty_tombstones_batch(schema: Arc<Schema>) -> RecordBatch {
        let empty_strings: Vec<Option<&str>> = vec![];
        let empty_timestamps: Vec<i64> = vec![];

        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(empty_strings.clone())),
                Arc::new(
                    TimestampMicrosecondArray::from(empty_timestamps.clone()).with_timezone("UTC"),
                ),
                Arc::new(StringArray::from(empty_strings.clone())),
                Arc::new(StringArray::from(empty_strings.clone())),
                Arc::new(
                    TimestampMicrosecondArray::from(empty_timestamps.clone()).with_timezone("UTC"),
                ),
                Arc::new(StringArray::from(empty_strings.clone())),
                Arc::new(StringArray::from(empty_strings)),
            ],
        )
        .expect("Schema matches columns")
    }

    pub async fn create_vector_index(&self) -> Result<()> {
        let table = self
            .memories_table
            .as_ref()
            .ok_or_else(|| MnemoError::Storage("Memories table not initialized".to_string()))?;

        let row_count = table
            .count_rows(None)
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to count rows: {e}")))?;

        // IVF-PQ requires at least 256 rows for training
        if row_count < 256 {
            return Ok(());
        }

        let ivf_pq = IvfPqIndexBuilder::default()
            .num_partitions(256)
            .num_sub_vectors(16);

        table
            .create_index(&["embedding"], Index::IvfPq(ivf_pq))
            .execute()
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to create vector index: {e}")))?;

        Ok(())
    }

    pub async fn open_memories_table(&mut self) -> Result<()> {
        let table = self
            .connection
            .open_table(MEMORIES_TABLE)
            .execute()
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to open memories table: {e}")))?;

        self.memories_table = Some(table);
        Ok(())
    }

    pub async fn open_tombstones_table(&mut self) -> Result<()> {
        let table = self
            .connection
            .open_table(TOMBSTONES_TABLE)
            .execute()
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to open tombstones table: {e}")))?;

        self.tombstones_table = Some(table);
        Ok(())
    }

    pub async fn table_exists(&self, name: &str) -> Result<bool> {
        let names = self
            .connection
            .table_names()
            .execute()
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to list tables: {e}")))?;

        Ok(names.contains(&name.to_string()))
    }

    pub fn memories_table(&self) -> Option<&Table> {
        self.memories_table.as_ref()
    }

    pub fn tombstones_table(&self) -> Option<&Table> {
        self.tombstones_table.as_ref()
    }

    /// Convert a Memory struct to an Arrow RecordBatch
    fn memory_to_batch(memory: &Memory, schema: Arc<Schema>) -> Result<RecordBatch> {
        Self::memories_to_batch(&[memory.clone()], schema)
    }

    /// Convert multiple Memory structs to an Arrow RecordBatch
    fn memories_to_batch(memories: &[Memory], schema: Arc<Schema>) -> Result<RecordBatch> {
        let ids: Vec<String> = memories.iter().map(|m| m.id.to_string()).collect();
        let id_refs: Vec<&str> = ids.iter().map(String::as_str).collect();

        let contents: Vec<&str> = memories.iter().map(|m| m.content.as_str()).collect();

        let embeddings: Vec<Option<Vec<Option<f32>>>> = memories
            .iter()
            .map(|m| Some(m.embedding.iter().map(|&v| Some(v)).collect()))
            .collect();

        let memory_types: Vec<&str> = memories
            .iter()
            .map(|m| match m.memory_type {
                MemoryType::Episodic => "Episodic",
                MemoryType::Semantic => "Semantic",
                MemoryType::Procedural => "Procedural",
            })
            .collect();

        let weights: Vec<f32> = memories.iter().map(|m| m.weight).collect();

        let created_at: Vec<i64> = memories
            .iter()
            .map(|m| m.created_at.timestamp_micros())
            .collect();

        let last_accessed: Vec<i64> = memories
            .iter()
            .map(|m| m.last_accessed.timestamp_micros())
            .collect();

        let access_counts: Vec<i32> = memories.iter().map(|m| m.access_count as i32).collect();

        let conversation_ids: Vec<Option<&str>> = memories
            .iter()
            .map(|m| m.conversation_id.as_deref())
            .collect();

        let sources: Vec<&str> = memories
            .iter()
            .map(|m| match m.source {
                MemorySource::Conversation => "Conversation",
                MemorySource::File => "File",
                MemorySource::Web => "Web",
                MemorySource::Manual => "Manual",
            })
            .collect();

        let tiers: Vec<&str> = memories
            .iter()
            .map(|m| match m.tier {
                StorageTier::Hot => "Hot",
                StorageTier::Warm => "Warm",
                StorageTier::Cold => "Cold",
            })
            .collect();

        let compressions: Vec<&str> = memories
            .iter()
            .map(|m| match m.compression {
                CompressionLevel::Full => "Full",
                CompressionLevel::Summary => "Summary",
                CompressionLevel::Keywords => "Keywords",
                CompressionLevel::Hash => "Hash",
            })
            .collect();

        let entities: Vec<String> = memories.iter().map(|m| m.entities.join(",")).collect();
        let entity_refs: Vec<&str> = entities.iter().map(String::as_str).collect();

        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(id_refs)),
                Arc::new(StringArray::from(contents)),
                Arc::new(FixedSizeListArray::from_iter_primitive::<
                    arrow_array::types::Float32Type,
                    _,
                    _,
                >(embeddings, EMBEDDING_DIMENSIONS)),
                Arc::new(StringArray::from(memory_types)),
                Arc::new(Float32Array::from(weights)),
                Arc::new(TimestampMicrosecondArray::from(created_at).with_timezone("UTC")),
                Arc::new(TimestampMicrosecondArray::from(last_accessed).with_timezone("UTC")),
                Arc::new(Int32Array::from(access_counts)),
                Arc::new(StringArray::from(conversation_ids)),
                Arc::new(StringArray::from(sources)),
                Arc::new(StringArray::from(tiers)),
                Arc::new(StringArray::from(compressions)),
                Arc::new(StringArray::from(entity_refs)),
            ],
        )
        .map_err(|e| MnemoError::Storage(format!("Failed to create RecordBatch: {e}")))
    }

    /// Convert an Arrow RecordBatch row back to a Memory struct
    fn batch_to_memory(batch: &RecordBatch, row: usize) -> Result<Memory> {
        let id_array = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| MnemoError::Storage("Failed to get id column".to_string()))?;

        let content_array = batch
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| MnemoError::Storage("Failed to get content column".to_string()))?;

        let embedding_array = batch
            .column(2)
            .as_any()
            .downcast_ref::<FixedSizeListArray>()
            .ok_or_else(|| MnemoError::Storage("Failed to get embedding column".to_string()))?;

        let memory_type_array = batch
            .column(3)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| MnemoError::Storage("Failed to get memory_type column".to_string()))?;

        let weight_array = batch
            .column(4)
            .as_any()
            .downcast_ref::<Float32Array>()
            .ok_or_else(|| MnemoError::Storage("Failed to get weight column".to_string()))?;

        let created_at_array = batch
            .column(5)
            .as_any()
            .downcast_ref::<TimestampMicrosecondArray>()
            .ok_or_else(|| MnemoError::Storage("Failed to get created_at column".to_string()))?;

        let last_accessed_array = batch
            .column(6)
            .as_any()
            .downcast_ref::<TimestampMicrosecondArray>()
            .ok_or_else(|| MnemoError::Storage("Failed to get last_accessed column".to_string()))?;

        let access_count_array = batch
            .column(7)
            .as_any()
            .downcast_ref::<Int32Array>()
            .ok_or_else(|| MnemoError::Storage("Failed to get access_count column".to_string()))?;

        let conversation_id_array = batch
            .column(8)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| {
                MnemoError::Storage("Failed to get conversation_id column".to_string())
            })?;

        let source_array = batch
            .column(9)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| MnemoError::Storage("Failed to get source column".to_string()))?;

        let tier_array = batch
            .column(10)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| MnemoError::Storage("Failed to get tier column".to_string()))?;

        let compression_array = batch
            .column(11)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| MnemoError::Storage("Failed to get compression column".to_string()))?;

        let entities_array = batch
            .column(12)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| MnemoError::Storage("Failed to get entities column".to_string()))?;

        // Parse ID
        let id = Uuid::parse_str(id_array.value(row))
            .map_err(|e| MnemoError::Storage(format!("Failed to parse UUID: {e}")))?;

        // Get content
        let content = content_array.value(row).to_string();

        // Get embedding
        let embedding_list = embedding_array.value(row);
        let embedding_values = embedding_list
            .as_any()
            .downcast_ref::<Float32Array>()
            .ok_or_else(|| MnemoError::Storage("Failed to get embedding values".to_string()))?;
        let embedding: Vec<f32> = (0..embedding_values.len())
            .map(|i| embedding_values.value(i))
            .collect();

        // Parse memory type
        let memory_type = match memory_type_array.value(row) {
            "Episodic" => MemoryType::Episodic,
            "Semantic" => MemoryType::Semantic,
            "Procedural" => MemoryType::Procedural,
            other => return Err(MnemoError::Storage(format!("Unknown memory type: {other}"))),
        };

        // Get weight
        let weight = weight_array.value(row);

        // Parse timestamps
        let created_at = Utc
            .timestamp_micros(created_at_array.value(row))
            .single()
            .ok_or_else(|| {
                MnemoError::Storage("Failed to parse created_at timestamp".to_string())
            })?;

        let last_accessed = Utc
            .timestamp_micros(last_accessed_array.value(row))
            .single()
            .ok_or_else(|| {
                MnemoError::Storage("Failed to parse last_accessed timestamp".to_string())
            })?;

        // Get access count
        let access_count = access_count_array.value(row) as u32;

        // Get optional conversation_id
        let conversation_id = if conversation_id_array.is_null(row) {
            None
        } else {
            let value = conversation_id_array.value(row);
            if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        };

        // Parse source
        let source = match source_array.value(row) {
            "Conversation" => MemorySource::Conversation,
            "File" => MemorySource::File,
            "Web" => MemorySource::Web,
            "Manual" => MemorySource::Manual,
            other => {
                return Err(MnemoError::Storage(format!(
                    "Unknown memory source: {other}"
                )));
            }
        };

        // Parse tier
        let tier = match tier_array.value(row) {
            "Hot" => StorageTier::Hot,
            "Warm" => StorageTier::Warm,
            "Cold" => StorageTier::Cold,
            other => {
                return Err(MnemoError::Storage(format!(
                    "Unknown storage tier: {other}"
                )));
            }
        };

        // Parse compression
        let compression = match compression_array.value(row) {
            "Full" => CompressionLevel::Full,
            "Summary" => CompressionLevel::Summary,
            "Keywords" => CompressionLevel::Keywords,
            "Hash" => CompressionLevel::Hash,
            other => {
                return Err(MnemoError::Storage(format!(
                    "Unknown compression level: {other}"
                )));
            }
        };

        // Parse entities (comma-separated)
        let entities_str = entities_array.value(row);
        let entities = if entities_str.is_empty() {
            Vec::new()
        } else {
            entities_str.split(',').map(|s| s.to_string()).collect()
        };

        Ok(Memory {
            id,
            content,
            embedding,
            memory_type,
            weight,
            created_at,
            last_accessed,
            access_count,
            conversation_id,
            entities,
            source,
            tier,
            compression,
        })
    }

    /// Convert a Tombstone struct to an Arrow RecordBatch
    fn tombstone_to_batch(tombstone: &Tombstone, schema: Arc<Schema>) -> Result<RecordBatch> {
        Self::tombstones_to_batch(&[tombstone.clone()], schema)
    }

    /// Convert multiple Tombstone structs to an Arrow RecordBatch
    fn tombstones_to_batch(tombstones: &[Tombstone], schema: Arc<Schema>) -> Result<RecordBatch> {
        let original_ids: Vec<String> = tombstones
            .iter()
            .map(|t| t.original_id.to_string())
            .collect();
        let id_refs: Vec<&str> = original_ids.iter().map(String::as_str).collect();

        let evicted_at: Vec<i64> = tombstones
            .iter()
            .map(|t| t.evicted_at.timestamp_micros())
            .collect();

        let topics: Vec<String> = tombstones.iter().map(|t| t.topics.join(",")).collect();
        let topic_refs: Vec<&str> = topics.iter().map(String::as_str).collect();

        let participants: Vec<String> = tombstones
            .iter()
            .map(|t| t.participants.join(","))
            .collect();
        let participant_refs: Vec<&str> = participants.iter().map(String::as_str).collect();

        let approximate_dates: Vec<i64> = tombstones
            .iter()
            .map(|t| t.approximate_date.timestamp_micros())
            .collect();

        let reasons: Vec<&str> = tombstones
            .iter()
            .map(|t| match t.reason {
                EvictionReason::StoragePressure => "StoragePressure",
                EvictionReason::LowWeight => "LowWeight",
                EvictionReason::Superseded { .. } => "Superseded",
                EvictionReason::ManualDeletion => "ManualDeletion",
            })
            .collect();

        let reason_details: Vec<Option<String>> = tombstones
            .iter()
            .map(|t| match &t.reason {
                EvictionReason::Superseded { by } => Some(by.to_string()),
                _ => None,
            })
            .collect();
        let reason_detail_refs: Vec<Option<&str>> =
            reason_details.iter().map(|s| s.as_deref()).collect();

        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(StringArray::from(id_refs)),
                Arc::new(TimestampMicrosecondArray::from(evicted_at).with_timezone("UTC")),
                Arc::new(StringArray::from(topic_refs)),
                Arc::new(StringArray::from(participant_refs)),
                Arc::new(TimestampMicrosecondArray::from(approximate_dates).with_timezone("UTC")),
                Arc::new(StringArray::from(reasons)),
                Arc::new(StringArray::from(reason_detail_refs)),
            ],
        )
        .map_err(|e| MnemoError::Storage(format!("Failed to create tombstone RecordBatch: {e}")))
    }

    /// Convert an Arrow RecordBatch row back to a Tombstone struct
    fn batch_to_tombstone(batch: &RecordBatch, row: usize) -> Result<Tombstone> {
        let original_id_array = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| MnemoError::Storage("Failed to get original_id column".to_string()))?;

        let evicted_at_array = batch
            .column(1)
            .as_any()
            .downcast_ref::<TimestampMicrosecondArray>()
            .ok_or_else(|| MnemoError::Storage("Failed to get evicted_at column".to_string()))?;

        let topics_array = batch
            .column(2)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| MnemoError::Storage("Failed to get topics column".to_string()))?;

        let participants_array = batch
            .column(3)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| MnemoError::Storage("Failed to get participants column".to_string()))?;

        let approximate_date_array = batch
            .column(4)
            .as_any()
            .downcast_ref::<TimestampMicrosecondArray>()
            .ok_or_else(|| {
                MnemoError::Storage("Failed to get approximate_date column".to_string())
            })?;

        let reason_array = batch
            .column(5)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| MnemoError::Storage("Failed to get reason column".to_string()))?;

        let reason_details_array = batch
            .column(6)
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| {
                MnemoError::Storage("Failed to get reason_details column".to_string())
            })?;

        // Parse original_id
        let original_id = Uuid::parse_str(original_id_array.value(row))
            .map_err(|e| MnemoError::Storage(format!("Failed to parse UUID: {e}")))?;

        // Parse evicted_at
        let evicted_at = Utc
            .timestamp_micros(evicted_at_array.value(row))
            .single()
            .ok_or_else(|| {
                MnemoError::Storage("Failed to parse evicted_at timestamp".to_string())
            })?;

        // Parse topics (comma-separated)
        let topics_str = topics_array.value(row);
        let topics = if topics_str.is_empty() {
            Vec::new()
        } else {
            topics_str.split(',').map(|s| s.to_string()).collect()
        };

        // Parse participants (comma-separated)
        let participants_str = participants_array.value(row);
        let participants = if participants_str.is_empty() {
            Vec::new()
        } else {
            participants_str.split(',').map(|s| s.to_string()).collect()
        };

        // Parse approximate_date
        let approximate_date = Utc
            .timestamp_micros(approximate_date_array.value(row))
            .single()
            .ok_or_else(|| {
                MnemoError::Storage("Failed to parse approximate_date timestamp".to_string())
            })?;

        // Parse reason
        let reason_str = reason_array.value(row);
        let reason = match reason_str {
            "StoragePressure" => EvictionReason::StoragePressure,
            "LowWeight" => EvictionReason::LowWeight,
            "Superseded" => {
                let details = if reason_details_array.is_null(row) {
                    None
                } else {
                    let val = reason_details_array.value(row);
                    if val.is_empty() { None } else { Some(val) }
                };
                match details {
                    Some(by_str) => {
                        let by = Uuid::parse_str(by_str).map_err(|e| {
                            MnemoError::Storage(format!("Failed to parse superseded by UUID: {e}"))
                        })?;
                        EvictionReason::Superseded { by }
                    }
                    None => EvictionReason::Superseded { by: Uuid::nil() },
                }
            }
            "ManualDeletion" => EvictionReason::ManualDeletion,
            other => {
                return Err(MnemoError::Storage(format!(
                    "Unknown eviction reason: {other}"
                )));
            }
        };

        Ok(Tombstone {
            original_id,
            evicted_at,
            topics,
            participants,
            approximate_date,
            reason,
        })
    }

    /// Insert a single tombstone into the store
    pub async fn insert_tombstone(&self, tombstone: &Tombstone) -> Result<()> {
        let table = self
            .tombstones_table
            .as_ref()
            .ok_or_else(|| MnemoError::Storage("Tombstones table not initialized".to_string()))?;

        let schema = Self::tombstones_schema();
        let batch = Self::tombstone_to_batch(tombstone, schema.clone())?;
        let batches = RecordBatchIterator::new(vec![Ok(batch)], schema);

        table
            .add(Box::new(batches))
            .execute()
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to insert tombstone: {e}")))?;

        Ok(())
    }

    /// Get a tombstone by original memory ID
    pub async fn get_tombstone(&self, original_id: Uuid) -> Result<Option<Tombstone>> {
        let table = self
            .tombstones_table
            .as_ref()
            .ok_or_else(|| MnemoError::Storage("Tombstones table not initialized".to_string()))?;

        let stream = table
            .query()
            .only_if(format!("original_id = '{original_id}'"))
            .execute()
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to query tombstone: {e}")))?;

        let batches: Vec<RecordBatch> = stream
            .try_collect()
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to collect query results: {e}")))?;

        if batches.is_empty() {
            return Ok(None);
        }

        let batch = &batches[0];
        if batch.num_rows() == 0 {
            return Ok(None);
        }

        let tombstone = Self::batch_to_tombstone(batch, 0)?;
        Ok(Some(tombstone))
    }

    /// Search tombstones by topic (case-insensitive substring match)
    pub async fn search_tombstones_by_topic(&self, topic: &str) -> Result<Vec<Tombstone>> {
        let table = self
            .tombstones_table
            .as_ref()
            .ok_or_else(|| MnemoError::Storage("Tombstones table not initialized".to_string()))?;

        // Use SQL LIKE for substring matching (case-insensitive)
        let pattern = format!("%{}%", topic.to_lowercase());
        let stream = table
            .query()
            .only_if(format!("lower(topics) LIKE '{pattern}'"))
            .execute()
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to search tombstones: {e}")))?;

        let batches: Vec<RecordBatch> = stream
            .try_collect()
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to collect search results: {e}")))?;

        let mut tombstones = Vec::new();
        for batch in &batches {
            for row in 0..batch.num_rows() {
                let tombstone = Self::batch_to_tombstone(batch, row)?;
                tombstones.push(tombstone);
            }
        }

        Ok(tombstones)
    }

    /// List all tombstones
    pub async fn list_all_tombstones(&self) -> Result<Vec<Tombstone>> {
        let table = self
            .tombstones_table
            .as_ref()
            .ok_or_else(|| MnemoError::Storage("Tombstones table not initialized".to_string()))?;

        let stream = table
            .query()
            .execute()
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to list tombstones: {e}")))?;

        let batches: Vec<RecordBatch> = stream
            .try_collect()
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to collect tombstones: {e}")))?;

        let mut tombstones = Vec::new();
        for batch in &batches {
            for row in 0..batch.num_rows() {
                let tombstone = Self::batch_to_tombstone(batch, row)?;
                tombstones.push(tombstone);
            }
        }

        Ok(tombstones)
    }

    /// Insert a single memory into the store
    pub async fn insert(&self, memory: &Memory) -> Result<()> {
        let table = self
            .memories_table
            .as_ref()
            .ok_or_else(|| MnemoError::Storage("Memories table not initialized".to_string()))?;

        let schema = Self::memories_schema();
        let batch = Self::memory_to_batch(memory, schema.clone())?;
        let batches = RecordBatchIterator::new(vec![Ok(batch)], schema);

        table
            .add(Box::new(batches))
            .execute()
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to insert memory: {e}")))?;

        Ok(())
    }

    /// Insert multiple memories in batch
    pub async fn insert_batch(&self, memories: &[Memory]) -> Result<()> {
        if memories.is_empty() {
            return Ok(());
        }

        let table = self
            .memories_table
            .as_ref()
            .ok_or_else(|| MnemoError::Storage("Memories table not initialized".to_string()))?;

        let schema = Self::memories_schema();
        let batch = Self::memories_to_batch(memories, schema.clone())?;
        let batches = RecordBatchIterator::new(vec![Ok(batch)], schema);

        table
            .add(Box::new(batches))
            .execute()
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to insert memories: {e}")))?;

        Ok(())
    }

    /// Get a memory by ID
    pub async fn get(&self, id: Uuid) -> Result<Option<Memory>> {
        let table = self
            .memories_table
            .as_ref()
            .ok_or_else(|| MnemoError::Storage("Memories table not initialized".to_string()))?;

        let stream = table
            .query()
            .only_if(format!("id = '{id}'"))
            .execute()
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to query memory: {e}")))?;

        let batches: Vec<RecordBatch> = stream
            .try_collect()
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to collect query results: {e}")))?;

        if batches.is_empty() {
            return Ok(None);
        }

        let batch = &batches[0];
        if batch.num_rows() == 0 {
            return Ok(None);
        }

        let memory = Self::batch_to_memory(batch, 0)?;
        Ok(Some(memory))
    }

    /// Delete a memory by ID
    /// Returns true if a memory was deleted, false if not found
    pub async fn delete(&self, id: Uuid) -> Result<bool> {
        let table = self
            .memories_table
            .as_ref()
            .ok_or_else(|| MnemoError::Storage("Memories table not initialized".to_string()))?;

        // First check if the memory exists
        let exists = self.get(id).await?.is_some();

        if exists {
            table
                .delete(&format!("id = '{id}'"))
                .await
                .map_err(|e| MnemoError::Storage(format!("Failed to delete memory: {e}")))?;
        }

        Ok(exists)
    }

    /// Update access stats (increment count, update timestamp)
    pub async fn update_access(&self, id: Uuid) -> Result<()> {
        let table = self
            .memories_table
            .as_ref()
            .ok_or_else(|| MnemoError::Storage("Memories table not initialized".to_string()))?;

        let now = Utc::now().timestamp_micros();

        table
            .update()
            .only_if(format!("id = '{id}'"))
            .column("access_count", "access_count + 1")
            .column("last_accessed", format!("{now}"))
            .execute()
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to update access: {e}")))?;

        Ok(())
    }

    /// Update the storage tier of a memory
    pub async fn update_tier(&self, id: Uuid, tier: StorageTier) -> Result<()> {
        let table = self
            .memories_table
            .as_ref()
            .ok_or_else(|| MnemoError::Storage("Memories table not initialized".to_string()))?;

        let tier_str = match tier {
            StorageTier::Hot => "Hot",
            StorageTier::Warm => "Warm",
            StorageTier::Cold => "Cold",
        };

        table
            .update()
            .only_if(format!("id = '{id}'"))
            .column("tier", format!("'{tier_str}'"))
            .execute()
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to update tier: {e}")))?;

        Ok(())
    }

    /// Update the conversation_id of a memory
    pub async fn update_conversation_id(
        &self,
        id: Uuid,
        conversation_id: Option<String>,
    ) -> Result<bool> {
        let table = self
            .memories_table
            .as_ref()
            .ok_or_else(|| MnemoError::Storage("Memories table not initialized".to_string()))?;

        let conv_id_value = match conversation_id {
            Some(ref id) => format!("'{id}'"),
            None => "NULL".to_string(),
        };

        let update_result = table
            .update()
            .only_if(format!("id = '{id}'"))
            .column("conversation_id", conv_id_value)
            .execute()
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to update memory: {e}")))?;

        Ok(update_result.rows_updated > 0)
    }

    /// Search for similar memories using vector similarity (ANN search)
    pub async fn search(&self, embedding: &[f32], limit: usize) -> Result<Vec<Memory>> {
        self.search_filtered(embedding, &MemoryFilter::default(), limit)
            .await
    }

    /// Search for similar memories with filter criteria
    pub async fn search_filtered(
        &self,
        embedding: &[f32],
        filter: &MemoryFilter,
        limit: usize,
    ) -> Result<Vec<Memory>> {
        let table = self
            .memories_table
            .as_ref()
            .ok_or_else(|| MnemoError::Storage("Memories table not initialized".to_string()))?;

        let mut query = table
            .query()
            .nearest_to(embedding)
            .map_err(|e| MnemoError::Storage(format!("Failed to create vector query: {e}")))?
            .limit(limit);

        if let Some(sql_filter) = filter.to_sql_clause() {
            query = query.only_if(sql_filter);
        }

        let stream = query
            .execute()
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to execute search: {e}")))?;

        let batches: Vec<RecordBatch> = stream
            .try_collect()
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to collect search results: {e}")))?;

        let mut memories = Vec::new();
        for batch in &batches {
            for row in 0..batch.num_rows() {
                let memory = Self::batch_to_memory(batch, row)?;
                memories.push(memory);
            }
        }

        Ok(memories)
    }

    /// List all memories in a specific storage tier
    pub async fn list_by_tier(&self, tier: StorageTier) -> Result<Vec<Memory>> {
        let table = self
            .memories_table
            .as_ref()
            .ok_or_else(|| MnemoError::Storage("Memories table not initialized".to_string()))?;

        let tier_str = match tier {
            StorageTier::Hot => "Hot",
            StorageTier::Warm => "Warm",
            StorageTier::Cold => "Cold",
        };

        let stream = table
            .query()
            .only_if(format!("tier = '{tier_str}'"))
            .execute()
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to query by tier: {e}")))?;

        let batches: Vec<RecordBatch> = stream
            .try_collect()
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to collect tier results: {e}")))?;

        let mut memories = Vec::new();
        for batch in &batches {
            for row in 0..batch.num_rows() {
                let memory = Self::batch_to_memory(batch, row)?;
                memories.push(memory);
            }
        }

        Ok(memories)
    }

    /// Count memories in a specific storage tier
    pub async fn count_by_tier(&self, tier: StorageTier) -> Result<usize> {
        let table = self
            .memories_table
            .as_ref()
            .ok_or_else(|| MnemoError::Storage("Memories table not initialized".to_string()))?;

        let tier_str = match tier {
            StorageTier::Hot => "Hot",
            StorageTier::Warm => "Warm",
            StorageTier::Cold => "Cold",
        };

        let count = table
            .count_rows(Some(format!("tier = '{tier_str}'")))
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to count by tier: {e}")))?;

        Ok(count)
    }

    /// Get the total number of memories across all tiers
    pub async fn total_count(&self) -> Result<usize> {
        let table = self
            .memories_table
            .as_ref()
            .ok_or_else(|| MnemoError::Storage("Memories table not initialized".to_string()))?;

        let count = table
            .count_rows(None)
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to count memories: {e}")))?;

        Ok(count)
    }

    /// Update content and compression level of a memory
    pub async fn update_compression(
        &self,
        id: Uuid,
        content: &str,
        compression: CompressionLevel,
    ) -> Result<()> {
        let table = self
            .memories_table
            .as_ref()
            .ok_or_else(|| MnemoError::Storage("Memories table not initialized".to_string()))?;

        let compression_str = match compression {
            CompressionLevel::Full => "Full",
            CompressionLevel::Summary => "Summary",
            CompressionLevel::Keywords => "Keywords",
            CompressionLevel::Hash => "Hash",
        };

        let escaped_content = content.replace('\'', "''");

        table
            .update()
            .only_if(format!("id = '{id}'"))
            .column("content", format!("'{escaped_content}'"))
            .column("compression", format!("'{compression_str}'"))
            .execute()
            .await
            .map_err(|e| MnemoError::Storage(format!("Failed to update compression: {e}")))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connect_and_create_table() {
        let temp_dir = tempfile::tempdir().unwrap();
        let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();

        assert!(!store.table_exists(MEMORIES_TABLE).await.unwrap());

        store.create_memories_table().await.unwrap();

        assert!(store.table_exists(MEMORIES_TABLE).await.unwrap());
        assert!(store.memories_table().is_some());
    }

    #[tokio::test]
    async fn test_open_existing_table() {
        let temp_dir = tempfile::tempdir().unwrap();

        {
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();
        }

        let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
        store.open_memories_table().await.unwrap();

        assert!(store.memories_table().is_some());
    }

    #[tokio::test]
    async fn test_schema_has_correct_fields() {
        let schema = LanceStore::memories_schema();

        assert_eq!(schema.fields().len(), 13);

        let field_names: Vec<&str> = schema.fields().iter().map(|f| f.name().as_str()).collect();
        assert!(field_names.contains(&"id"));
        assert!(field_names.contains(&"content"));
        assert!(field_names.contains(&"embedding"));
        assert!(field_names.contains(&"memory_type"));
        assert!(field_names.contains(&"weight"));
        assert!(field_names.contains(&"created_at"));
        assert!(field_names.contains(&"last_accessed"));
        assert!(field_names.contains(&"access_count"));
        assert!(field_names.contains(&"conversation_id"));
        assert!(field_names.contains(&"source"));
        assert!(field_names.contains(&"tier"));
        assert!(field_names.contains(&"compression"));
        assert!(field_names.contains(&"entities"));
    }

    #[tokio::test]
    async fn test_embedding_field_dimensions() {
        let schema = LanceStore::memories_schema();
        let embedding_field = schema.field_with_name("embedding").unwrap();

        match embedding_field.data_type() {
            DataType::FixedSizeList(_, size) => {
                assert_eq!(*size, EMBEDDING_DIMENSIONS);
            }
            _ => panic!("Expected FixedSizeList type for embedding field"),
        }
    }

    mod crud {
        use super::*;

        fn create_test_memory(content: &str) -> Memory {
            Memory::new(
                content.to_string(),
                vec![0.1; 384],
                MemoryType::Semantic,
                MemorySource::Manual,
            )
        }

        #[tokio::test]
        async fn test_insert_and_get() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory("Test memory content");
            let id = memory.id;

            store.insert(&memory).await.unwrap();

            let retrieved = store.get(id).await.unwrap();
            assert!(retrieved.is_some());

            let retrieved = retrieved.unwrap();
            assert_eq!(retrieved.id, memory.id);
            assert_eq!(retrieved.content, memory.content);
            assert_eq!(retrieved.embedding.len(), 384);
            assert_eq!(retrieved.memory_type, memory.memory_type);
            assert_eq!(retrieved.source, memory.source);
        }

        #[tokio::test]
        async fn test_insert_batch() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memories: Vec<Memory> = (0..3)
                .map(|i| create_test_memory(&format!("Memory {i}")))
                .collect();

            let ids: Vec<Uuid> = memories.iter().map(|m| m.id).collect();

            store.insert_batch(&memories).await.unwrap();

            for (i, id) in ids.iter().enumerate() {
                let retrieved = store.get(*id).await.unwrap();
                assert!(retrieved.is_some());
                assert_eq!(retrieved.unwrap().content, format!("Memory {i}"));
            }
        }

        #[tokio::test]
        async fn test_get_nonexistent_returns_none() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let nonexistent_id = Uuid::new_v4();
            let result = store.get(nonexistent_id).await.unwrap();
            assert!(result.is_none());
        }

        #[tokio::test]
        async fn test_delete() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory("To be deleted");
            let id = memory.id;

            store.insert(&memory).await.unwrap();

            assert!(store.get(id).await.unwrap().is_some());

            let deleted = store.delete(id).await.unwrap();
            assert!(deleted);

            assert!(store.get(id).await.unwrap().is_none());
        }

        #[tokio::test]
        async fn test_delete_nonexistent_returns_false() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let nonexistent_id = Uuid::new_v4();
            let deleted = store.delete(nonexistent_id).await.unwrap();
            assert!(!deleted);
        }

        #[tokio::test]
        async fn test_update_access() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let memory = create_test_memory("Access test");
            let id = memory.id;
            let original_access_count = memory.access_count;
            let original_last_accessed = memory.last_accessed;

            store.insert(&memory).await.unwrap();

            tokio::time::sleep(std::time::Duration::from_millis(10)).await;

            store.update_access(id).await.unwrap();

            let updated = store.get(id).await.unwrap().unwrap();
            assert_eq!(updated.access_count, original_access_count + 1);
            assert!(updated.last_accessed > original_last_accessed);
        }

        #[tokio::test]
        async fn test_roundtrip_preserves_all_fields() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let mut memory = Memory::new(
                "Complete memory test".to_string(),
                vec![0.5; 384],
                MemoryType::Episodic,
                MemorySource::Conversation,
            );
            memory.weight = 0.75;
            memory.conversation_id = Some("conv-123".to_string());
            memory.tier = StorageTier::Warm;
            memory.compression = CompressionLevel::Summary;

            let id = memory.id;

            store.insert(&memory).await.unwrap();

            let retrieved = store.get(id).await.unwrap().unwrap();

            assert_eq!(retrieved.id, memory.id);
            assert_eq!(retrieved.content, memory.content);
            assert_eq!(retrieved.memory_type, memory.memory_type);
            assert_eq!(retrieved.weight, memory.weight);
            assert_eq!(retrieved.conversation_id, memory.conversation_id);
            assert_eq!(retrieved.source, memory.source);
            assert_eq!(retrieved.tier, memory.tier);
            assert_eq!(retrieved.compression, memory.compression);
        }
    }

    mod search {
        use super::*;

        fn create_memory_with_embedding(
            content: &str,
            embedding: Vec<f32>,
            memory_type: MemoryType,
        ) -> Memory {
            Memory::new(
                content.to_string(),
                embedding,
                memory_type,
                MemorySource::Manual,
            )
        }

        fn similar_embedding(base: &[f32], variation: f32) -> Vec<f32> {
            base.iter().map(|v| v + variation).collect()
        }

        #[tokio::test]
        async fn test_search_returns_similar_memories() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let base_embedding: Vec<f32> = vec![0.5; 384];

            let memories = vec![
                create_memory_with_embedding(
                    "Similar 1",
                    similar_embedding(&base_embedding, 0.01),
                    MemoryType::Semantic,
                ),
                create_memory_with_embedding(
                    "Similar 2",
                    similar_embedding(&base_embedding, 0.02),
                    MemoryType::Semantic,
                ),
                create_memory_with_embedding("Different", vec![0.9; 384], MemoryType::Semantic),
            ];
            store.insert_batch(&memories).await.unwrap();

            let results = store.search(&base_embedding, 10).await.unwrap();

            assert_eq!(results.len(), 3);
            assert!(results[0].content.starts_with("Similar"));
        }

        #[tokio::test]
        async fn test_search_respects_limit() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let base_embedding: Vec<f32> = vec![0.5; 384];
            let memories: Vec<Memory> = (0..5)
                .map(|i| {
                    create_memory_with_embedding(
                        &format!("Memory {i}"),
                        similar_embedding(&base_embedding, i as f32 * 0.01),
                        MemoryType::Semantic,
                    )
                })
                .collect();
            store.insert_batch(&memories).await.unwrap();

            let results = store.search(&base_embedding, 2).await.unwrap();

            assert_eq!(results.len(), 2);
        }

        #[tokio::test]
        async fn test_search_filtered_by_memory_type() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let base_embedding: Vec<f32> = vec![0.5; 384];
            let memories = vec![
                create_memory_with_embedding(
                    "Semantic 1",
                    base_embedding.clone(),
                    MemoryType::Semantic,
                ),
                create_memory_with_embedding(
                    "Episodic 1",
                    base_embedding.clone(),
                    MemoryType::Episodic,
                ),
                create_memory_with_embedding(
                    "Semantic 2",
                    base_embedding.clone(),
                    MemoryType::Semantic,
                ),
            ];
            store.insert_batch(&memories).await.unwrap();

            let filter = MemoryFilter::new().with_memory_types(vec![MemoryType::Semantic]);
            let results = store
                .search_filtered(&base_embedding, &filter, 10)
                .await
                .unwrap();

            assert_eq!(results.len(), 2);
            for memory in &results {
                assert_eq!(memory.memory_type, MemoryType::Semantic);
            }
        }

        #[tokio::test]
        async fn test_search_filtered_by_min_weight() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let base_embedding: Vec<f32> = vec![0.5; 384];

            let mut low_weight = create_memory_with_embedding(
                "Low weight",
                base_embedding.clone(),
                MemoryType::Semantic,
            );
            low_weight.weight = 0.3;

            let mut high_weight = create_memory_with_embedding(
                "High weight",
                base_embedding.clone(),
                MemoryType::Semantic,
            );
            high_weight.weight = 0.8;

            store.insert(&low_weight).await.unwrap();
            store.insert(&high_weight).await.unwrap();

            let filter = MemoryFilter::new().with_min_weight(0.5);
            let results = store
                .search_filtered(&base_embedding, &filter, 10)
                .await
                .unwrap();

            assert_eq!(results.len(), 1);
            assert_eq!(results[0].content, "High weight");
        }

        #[tokio::test]
        async fn test_search_filtered_by_conversation_id() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let base_embedding: Vec<f32> = vec![0.5; 384];

            let mut conv_a = create_memory_with_embedding(
                "Conv A",
                base_embedding.clone(),
                MemoryType::Episodic,
            );
            conv_a.conversation_id = Some("conv-a".to_string());

            let mut conv_b = create_memory_with_embedding(
                "Conv B",
                base_embedding.clone(),
                MemoryType::Episodic,
            );
            conv_b.conversation_id = Some("conv-b".to_string());

            store.insert(&conv_a).await.unwrap();
            store.insert(&conv_b).await.unwrap();

            let filter = MemoryFilter::new().with_conversation_id("conv-a".to_string());
            let results = store
                .search_filtered(&base_embedding, &filter, 10)
                .await
                .unwrap();

            assert_eq!(results.len(), 1);
            assert_eq!(results[0].conversation_id, Some("conv-a".to_string()));
        }

        #[tokio::test]
        async fn test_search_with_combined_filters() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let base_embedding: Vec<f32> = vec![0.5; 384];

            let mut m1 =
                create_memory_with_embedding("Match", base_embedding.clone(), MemoryType::Semantic);
            m1.weight = 0.8;
            m1.conversation_id = Some("conv-1".to_string());

            let mut m2 = create_memory_with_embedding(
                "Wrong type",
                base_embedding.clone(),
                MemoryType::Episodic,
            );
            m2.weight = 0.8;
            m2.conversation_id = Some("conv-1".to_string());

            let mut m3 = create_memory_with_embedding(
                "Low weight",
                base_embedding.clone(),
                MemoryType::Semantic,
            );
            m3.weight = 0.2;
            m3.conversation_id = Some("conv-1".to_string());

            store.insert_batch(&[m1, m2, m3]).await.unwrap();

            let filter = MemoryFilter::new()
                .with_memory_types(vec![MemoryType::Semantic])
                .with_min_weight(0.5);
            let results = store
                .search_filtered(&base_embedding, &filter, 10)
                .await
                .unwrap();

            assert_eq!(results.len(), 1);
            assert_eq!(results[0].content, "Match");
        }

        #[tokio::test]
        async fn test_search_empty_results() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let base_embedding: Vec<f32> = vec![0.5; 384];
            let results = store.search(&base_embedding, 10).await.unwrap();

            assert!(results.is_empty());
        }

        #[tokio::test]
        async fn test_search_latency_reasonable() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_memories_table().await.unwrap();

            let base_embedding: Vec<f32> = vec![0.5; 384];
            let memories: Vec<Memory> = (0..100)
                .map(|i| {
                    create_memory_with_embedding(
                        &format!("Memory {i}"),
                        similar_embedding(&base_embedding, (i as f32) * 0.001),
                        MemoryType::Semantic,
                    )
                })
                .collect();
            store.insert_batch(&memories).await.unwrap();

            let start = std::time::Instant::now();
            let _results = store.search(&base_embedding, 10).await.unwrap();
            let elapsed = start.elapsed();

            assert!(
                elapsed.as_millis() < 1000,
                "Search took too long: {elapsed:?}"
            );
        }
    }

    mod tombstones {
        use super::*;
        use crate::memory::tombstone::{EvictionReason, Tombstone};

        #[tokio::test]
        async fn test_create_and_open_tombstones_table() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();

            assert!(store.tombstones_table().is_none());

            store.create_tombstones_table().await.unwrap();

            assert!(store.tombstones_table().is_some());
        }

        #[tokio::test]
        async fn test_insert_and_get_tombstone() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_tombstones_table().await.unwrap();

            let original_id = Uuid::new_v4();
            let tombstone = Tombstone::new(
                original_id,
                vec!["rust".to_string(), "programming".to_string()],
                vec!["alice".to_string()],
                Utc::now(),
                EvictionReason::StoragePressure,
            );

            store.insert_tombstone(&tombstone).await.unwrap();

            let retrieved = store.get_tombstone(original_id).await.unwrap();
            assert!(retrieved.is_some());

            let retrieved = retrieved.unwrap();
            assert_eq!(retrieved.original_id, original_id);
            assert_eq!(retrieved.topics, vec!["rust", "programming"]);
            assert_eq!(retrieved.participants, vec!["alice"]);
            assert!(matches!(retrieved.reason, EvictionReason::StoragePressure));
        }

        #[tokio::test]
        async fn test_get_nonexistent_tombstone_returns_none() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_tombstones_table().await.unwrap();

            let nonexistent_id = Uuid::new_v4();
            let result = store.get_tombstone(nonexistent_id).await.unwrap();
            assert!(result.is_none());
        }

        #[tokio::test]
        async fn test_search_tombstones_by_topic() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_tombstones_table().await.unwrap();

            let tombstone1 = Tombstone::new(
                Uuid::new_v4(),
                vec!["machine-learning".to_string(), "python".to_string()],
                vec![],
                Utc::now(),
                EvictionReason::LowWeight,
            );

            let tombstone2 = Tombstone::new(
                Uuid::new_v4(),
                vec!["rust".to_string(), "systems-programming".to_string()],
                vec![],
                Utc::now(),
                EvictionReason::StoragePressure,
            );

            let tombstone3 = Tombstone::new(
                Uuid::new_v4(),
                vec!["python".to_string(), "django".to_string()],
                vec![],
                Utc::now(),
                EvictionReason::LowWeight,
            );

            store.insert_tombstone(&tombstone1).await.unwrap();
            store.insert_tombstone(&tombstone2).await.unwrap();
            store.insert_tombstone(&tombstone3).await.unwrap();

            let results = store.search_tombstones_by_topic("python").await.unwrap();
            assert_eq!(results.len(), 2);

            let results = store.search_tombstones_by_topic("rust").await.unwrap();
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].topics, vec!["rust", "systems-programming"]);

            let results = store
                .search_tombstones_by_topic("nonexistent")
                .await
                .unwrap();
            assert!(results.is_empty());
        }

        #[tokio::test]
        async fn test_list_all_tombstones() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_tombstones_table().await.unwrap();

            let tombstones: Vec<Tombstone> = (0..3)
                .map(|i| {
                    Tombstone::new(
                        Uuid::new_v4(),
                        vec![format!("topic-{}", i)],
                        vec![],
                        Utc::now(),
                        EvictionReason::StoragePressure,
                    )
                })
                .collect();

            for tombstone in &tombstones {
                store.insert_tombstone(tombstone).await.unwrap();
            }

            let all_tombstones = store.list_all_tombstones().await.unwrap();
            assert_eq!(all_tombstones.len(), 3);
        }

        #[tokio::test]
        async fn test_tombstone_roundtrip_preserves_all_fields() {
            let temp_dir = tempfile::tempdir().unwrap();
            let mut store = LanceStore::connect(temp_dir.path()).await.unwrap();
            store.create_tombstones_table().await.unwrap();

            let original_id = Uuid::new_v4();
            let superseded_by = Uuid::new_v4();
            let created_at = Utc::now();

            let tombstone = Tombstone {
                original_id,
                evicted_at: Utc::now(),
                topics: vec!["topic1".to_string(), "topic2".to_string()],
                participants: vec!["participant1".to_string()],
                approximate_date: created_at,
                reason: EvictionReason::Superseded { by: superseded_by },
            };

            store.insert_tombstone(&tombstone).await.unwrap();

            let retrieved = store.get_tombstone(original_id).await.unwrap().unwrap();

            assert_eq!(retrieved.original_id, original_id);
            assert_eq!(retrieved.topics, vec!["topic1", "topic2"]);
            assert_eq!(retrieved.participants, vec!["participant1"]);
            assert_eq!(retrieved.approximate_date, created_at);

            match retrieved.reason {
                EvictionReason::Superseded { by } => assert_eq!(by, superseded_by),
                _ => panic!("Expected Superseded reason"),
            }
        }
    }
}
