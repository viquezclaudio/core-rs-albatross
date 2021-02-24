use nimiq_block_albatross::Block;
use nimiq_blockchain_albatross::{
    AbstractBlockchain, Blockchain, ChainInfo, ChainOrdering, PushError, PushResult,
};
use nimiq_hash::Blake2bHash;

use crate::blockchain::NanoBlockchain;

/// Implements methods to push blocks into the chain. This is used when the node has already synced
/// and is just receiving newly produced blocks. It is also used for the final phase of syncing,
/// when the node is just receiving micro blocks.
impl NanoBlockchain {
    /// Pushes a block into the chain.
    pub fn push(&mut self, block: Block) -> Result<PushResult, PushError> {
        // Check if we already know this block.
        if self.get_chain_info(&block.hash(), false).is_some() {
            return Ok(PushResult::Known);
        }

        // Check if we have this block's parent.
        let prev_info = self
            .get_chain_info(&block.parent_hash(), false)
            .ok_or_else(|| PushError::Orphan)?;

        // Calculate chain ordering.
        let chain_order = ChainOrdering::order_chains(self, &block, &prev_info);

        // If it is an inferior chain, we ignore it as it cannot become better at any point in time.
        if chain_order == ChainOrdering::Inferior {
            return Ok(PushResult::Ignored);
        }

        // Get the intended slot owner.
        let (validator, _) = self
            .get_slot_owner_at(block.block_number(), block.view_number())
            .expect("Failed to find slot owner!");

        let intended_slot_owner = validator.public_key.uncompress_unchecked();

        // Check the header.
        Blockchain::verify_block_header(self, &block.header(), &intended_slot_owner)?;

        // Check the justification.
        Blockchain::verify_block_justification(
            self,
            &block.header(),
            &block.justification(),
            &intended_slot_owner,
        )?;

        // Create the chaininfo for the new block.
        let chain_info = match ChainInfo::from_block(block, &prev_info) {
            Ok(v) => v,
            Err(_) => {
                return Err(PushError::InvalidSuccessor);
            }
        };

        // More chain ordering.
        match chain_order {
            ChainOrdering::Extend => {
                //return self.extend(chain_info.head.hash(), chain_info, prev_info);
            }
            ChainOrdering::Better => {
                //return self.rebranch(chain_info.head.hash(), chain_info);
            }
            ChainOrdering::Inferior => unreachable!(),
            ChainOrdering::Unknown => {}
        }

        // Otherwise, we are creating/extending a fork. Store ChainInfo.
        self.chain_store
            .write()
            .unwrap()
            .put_chain_info(chain_info.head.hash(), chain_info);

        Ok(PushResult::Forked)
    }

    // /// Extends the current main chain.
    // fn extend(
    //     &self,
    //     block_hash: Blake2bHash,
    //     mut chain_info: ChainInfo,
    //     mut prev_info: ChainInfo,
    // ) -> Result<PushResult, PushError> {
    //     let mut txn = WriteTransaction::new(&self.env);
    //
    //     let state = self.state.read();
    //
    //     // Check transactions against TransactionCache to prevent replay.
    //     // This is technically unnecessary for macro blocks, but it doesn't hurt either.
    //     if state.transaction_cache.contains_any(&chain_info.head) {
    //         warn!("Rejecting block - transaction already included");
    //         txn.abort();
    //         return Err(PushError::DuplicateTransaction);
    //     }
    //
    //     // Commit block to AccountsTree.
    //     if let Err(e) = self.commit_accounts(
    //         &state,
    //         &chain_info.head,
    //         prev_info.head.next_view_number(),
    //         &mut txn,
    //     ) {
    //         warn!("Rejecting block - commit failed: {:?}", e);
    //         txn.abort();
    //         #[cfg(feature = "metrics")]
    //         self.metrics.note_invalid_block();
    //         return Err(e);
    //     }
    //
    //     // Verify the state against the block.
    //     if let Err(e) = self.verify_block_state(&state, &chain_info.head, Some(&txn)) {
    //         warn!("Rejecting block - Bad state");
    //         return Err(e);
    //     }
    //
    //     drop(state);
    //
    //     chain_info.on_main_chain = true;
    //     prev_info.main_chain_successor = Some(chain_info.head.hash());
    //
    //     self.chain_store
    //         .put_chain_info(&mut txn, &block_hash, &chain_info, true);
    //     self.chain_store.put_chain_info(
    //         &mut txn,
    //         &chain_info.head.parent_hash(),
    //         &prev_info,
    //         false,
    //     );
    //     self.chain_store.set_head(&mut txn, &block_hash);
    //
    //     let is_election_block = policy::is_election_block_at(self.block_number() + 1);
    //
    //     // Acquire write lock & commit changes.
    //     let mut state = self.state.write();
    //     state.transaction_cache.push_block(&chain_info.head);
    //
    //     let mut is_macro = false;
    //     if let Block::Macro(ref macro_block) = chain_info.head {
    //         is_macro = true;
    //         state.macro_info = chain_info.clone();
    //         state.macro_head_hash = block_hash.clone();
    //
    //         if is_election_block {
    //             state.election_head = macro_block.clone();
    //             state.election_head_hash = block_hash.clone();
    //
    //             let old_slots = state.current_slots.take().unwrap();
    //             state.previous_slots.replace(old_slots);
    //
    //             let new_slots = macro_block.get_slots().unwrap();
    //             state.current_slots.replace(new_slots);
    //         }
    //     }
    //
    //     state.main_chain = chain_info;
    //     state.head_hash = block_hash.clone();
    //     txn.commit();
    //
    //     // Give up lock before notifying.
    //     drop(state);
    //
    //     if is_macro {
    //         if is_election_block {
    //             self.notifier
    //                 .read()
    //                 .notify(BlockchainEvent::EpochFinalized(block_hash));
    //         } else {
    //             self.notifier
    //                 .read()
    //                 .notify(BlockchainEvent::Finalized(block_hash));
    //         }
    //     } else {
    //         self.notifier
    //             .read()
    //             .notify(BlockchainEvent::Extended(block_hash));
    //     }
    //
    //     Ok(PushResult::Extended)
    // }
    //
    // /// Rebranches the current main chain.
    // fn rebranch(
    //     &self,
    //     block_hash: Blake2bHash,
    //     chain_info: ChainInfo,
    // ) -> Result<PushResult, PushError> {
    //     debug!(
    //         "Rebranching to fork {}, height #{}, view number {}",
    //         block_hash,
    //         chain_info.head.block_number(),
    //         chain_info.head.view_number()
    //     );
    //
    //     // Find the common ancestor between our current main chain and the fork chain.
    //     // Walk up the fork chain until we find a block that is part of the main chain.
    //     // Store the chain along the way.
    //     let read_txn = ReadTransaction::new(&self.env);
    //
    //     let mut fork_chain: Vec<(Blake2bHash, ChainInfo)> = vec![];
    //
    //     let mut current: (Blake2bHash, ChainInfo) = (block_hash, chain_info);
    //
    //     while !current.1.on_main_chain {
    //         // A fork can't contain a macro block. We already received that macro block, thus it must be on our
    //         // main chain.
    //         assert_eq!(
    //             current.1.head.ty(),
    //             BlockType::Micro,
    //             "Fork contains macro block"
    //         );
    //
    //         let prev_hash = current.1.head.parent_hash().clone();
    //
    //         let prev_info = self
    //             .chain_store
    //             .get_chain_info(&prev_hash, true, Some(&read_txn))
    //             .expect("Corrupted store: Failed to find fork predecessor while rebranching");
    //
    //         fork_chain.push(current);
    //
    //         current = (prev_hash, prev_info);
    //     }
    //
    //     debug!(
    //         "Found common ancestor {} at height #{}, {} blocks up",
    //         current.0,
    //         current.1.head.block_number(),
    //         fork_chain.len()
    //     );
    //
    //     // Revert AccountsTree & TransactionCache to the common ancestor state.
    //     let mut revert_chain: Vec<(Blake2bHash, ChainInfo)> = vec![];
    //     let mut ancestor = current;
    //
    //     let mut write_txn = WriteTransaction::new(&self.env);
    //     let mut cache_txn;
    //
    //     let state = self.state.upgradable_read();
    //     cache_txn = state.transaction_cache.clone();
    //     // TODO: Get rid of the .clone() here.
    //     current = (state.head_hash.clone(), state.main_chain.clone());
    //
    //     // Check if ancestor is in current epoch
    //     if ancestor.1.head.block_number() < state.macro_info.head.block_number() {
    //         info!("Ancestor is in finalized epoch");
    //         return Err(PushError::InvalidFork);
    //     }
    //
    //     while current.0 != ancestor.0 {
    //         match current.1.head {
    //             Block::Macro(_) => {
    //                 // Macro blocks are final, we can't revert across them. This should be checked
    //                 // before we start reverting.
    //                 panic!("Trying to rebranch across macro block");
    //             }
    //             Block::Micro(ref micro_block) => {
    //                 let prev_hash = micro_block.header.parent_hash.clone();
    //
    //                 let prev_info = self
    //                     .chain_store
    //                     .get_chain_info(&prev_hash, true, Some(&read_txn))
    //                     .expect("Corrupted store: Failed to find main chain predecessor while rebranching");
    //
    //                 self.revert_accounts(
    //                     &state.accounts,
    //                     &mut write_txn,
    //                     &micro_block,
    //                     prev_info.head.view_number(),
    //                 )?;
    //
    //                 cache_txn.revert_block(&current.1.head);
    //
    //                 assert_eq!(
    //                     prev_info.head.state_root(),
    //                     &state.accounts.hash(Some(&write_txn)),
    //                     "Failed to revert main chain while rebranching - inconsistent state"
    //                 );
    //
    //                 revert_chain.push(current);
    //
    //                 current = (prev_hash, prev_info);
    //             }
    //         }
    //     }
    //
    //     // Fetch missing blocks for TransactionCache.
    //     assert!(cache_txn.is_empty() || cache_txn.head_hash() == ancestor.0);
    //
    //     let start_hash = if cache_txn.is_empty() {
    //         ancestor.1.main_chain_successor.unwrap()
    //     } else {
    //         cache_txn.tail_hash()
    //     };
    //
    //     let blocks = self.chain_store.get_blocks_backward(
    //         &start_hash,
    //         cache_txn.missing_blocks(),
    //         true,
    //         Some(&read_txn),
    //     );
    //
    //     for block in blocks.iter() {
    //         cache_txn.prepend_block(block)
    //     }
    //
    //     assert_eq!(
    //         cache_txn.missing_blocks(),
    //         policy::TRANSACTION_VALIDITY_WINDOW.saturating_sub(ancestor.1.head.block_number() + 1)
    //     );
    //
    //     // Check each fork block against TransactionCache & commit to AccountsTree and SlashRegistry.
    //     let mut prev_view_number = ancestor.1.head.next_view_number();
    //
    //     let mut fork_iter = fork_chain.iter().rev();
    //
    //     while let Some(fork_block) = fork_iter.next() {
    //         match fork_block.1.head {
    //             Block::Macro(_) => unreachable!(),
    //             Block::Micro(ref micro_block) => {
    //                 let result = if !cache_txn.contains_any(&fork_block.1.head) {
    //                     self.commit_accounts(
    //                         &state,
    //                         &fork_block.1.head,
    //                         prev_view_number,
    //                         &mut write_txn,
    //                     )
    //                 } else {
    //                     Err(PushError::DuplicateTransaction)
    //                 };
    //
    //                 if let Err(e) = result {
    //                     warn!("Failed to apply fork block while rebranching - {:?}", e);
    //                     write_txn.abort();
    //
    //                     // Delete invalid fork blocks from store.
    //                     let mut write_txn = WriteTransaction::new(&self.env);
    //
    //                     for block in vec![fork_block].into_iter().chain(fork_iter) {
    //                         self.chain_store.remove_chain_info(
    //                             &mut write_txn,
    //                             &block.0,
    //                             micro_block.header.block_number,
    //                         )
    //                     }
    //
    //                     write_txn.commit();
    //
    //                     return Err(PushError::InvalidFork);
    //                 }
    //
    //                 cache_txn.push_block(&fork_block.1.head);
    //
    //                 prev_view_number = fork_block.1.head.next_view_number();
    //             }
    //         }
    //     }
    //
    //     // Fork looks good.
    //     // Drop read transaction.
    //     read_txn.close();
    //
    //     // Acquire write lock.
    //     let mut state = RwLockUpgradableReadGuard::upgrade(state);
    //
    //     // Unset onMainChain flag / mainChainSuccessor on the current main chain up to (excluding) the common ancestor.
    //     for reverted_block in revert_chain.iter_mut() {
    //         reverted_block.1.on_main_chain = false;
    //
    //         reverted_block.1.main_chain_successor = None;
    //
    //         self.chain_store.put_chain_info(
    //             &mut write_txn,
    //             &reverted_block.0,
    //             &reverted_block.1,
    //             false,
    //         );
    //     }
    //
    //     // Update the mainChainSuccessor of the common ancestor block.
    //     ancestor.1.main_chain_successor = Some(fork_chain.last().unwrap().0.clone());
    //     self.chain_store
    //         .put_chain_info(&mut write_txn, &ancestor.0, &ancestor.1, false);
    //
    //     // Set onMainChain flag / mainChainSuccessor on the fork.
    //     for i in (0..fork_chain.len()).rev() {
    //         let main_chain_successor = if i > 0 {
    //             Some(fork_chain[i - 1].0.clone())
    //         } else {
    //             None
    //         };
    //
    //         let fork_block = &mut fork_chain[i];
    //         fork_block.1.on_main_chain = true;
    //         fork_block.1.main_chain_successor = main_chain_successor;
    //
    //         // Include the body of the new block (at position 0).
    //         self.chain_store
    //             .put_chain_info(&mut write_txn, &fork_block.0, &fork_block.1, i == 0);
    //     }
    //
    //     // Commit transaction & update head.
    //     self.chain_store.set_head(&mut write_txn, &fork_chain[0].0);
    //
    //     state.transaction_cache = cache_txn;
    //
    //     state.main_chain = fork_chain[0].1.clone();
    //
    //     state.head_hash = fork_chain[0].0.clone();
    //
    //     write_txn.commit();
    //
    //     // Give up lock before notifying.
    //     drop(state);
    //
    //     let mut reverted_blocks = Vec::with_capacity(revert_chain.len());
    //
    //     for (hash, chain_info) in revert_chain.into_iter().rev() {
    //         reverted_blocks.push((hash, chain_info.head));
    //     }
    //
    //     let mut adopted_blocks = Vec::with_capacity(fork_chain.len());
    //
    //     for (hash, chain_info) in fork_chain.into_iter().rev() {
    //         adopted_blocks.push((hash, chain_info.head));
    //     }
    //
    //     let event = BlockchainEvent::Rebranched(reverted_blocks, adopted_blocks);
    //
    //     self.notifier.read().notify(event);
    //
    //     Ok(PushResult::Rebranched)
    // }
}