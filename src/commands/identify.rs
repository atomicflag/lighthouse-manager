use anyhow::Result;
use tracing::{error, info};

use crate::lighthouse::LighthouseVersion;

/// Identify a specific V2 lighthouse by index in the database.
/// This causes the lighthouse to blink its LED for visual identification.
pub async fn run(index: usize) -> Result<()> {
    let db = crate::storage::load()?;

    if index >= db.lighthouses.len() {
        anyhow::bail!(
            "Index {} out of range (database has {} entries). Use `lighthouse-manager list` to see available indices.",
            index,
            db.lighthouses.len()
        );
    }

    let lh = &db.lighthouses[index];

    if lh.version() != LighthouseVersion::V2 {
        anyhow::bail!(
            "Identify is only supported on V2 lighthouses. '{}' is a {}",
            lh.name,
            lh.version()
        );
    }

    info!(
        "Sending identify command to {} ({}), index [{}]...",
        lh.name, lh.address, index
    );

    let adapter = crate::bluetooth::get_adapter().await?;

    match tokio::time::timeout(std::time::Duration::from_secs(15), async {
        let conn = crate::bluetooth::connect_lighthouse(&adapter, &lh.address).await?;
        conn.identify(lh).await?;
        conn.disconnect().await;
        Ok::<(), anyhow::Error>(())
    })
    .await
    {
        Ok(Ok(())) => {
            info!("✓ {} should be blinking now.", lh.name);
        }
        Ok(Err(e)) => {
            error!("Failed to identify {}: {}", lh.name, e);
            return Err(e);
        }
        Err(_) => {
            anyhow::bail!(
                "Timed out while trying to identify {}. Is it powered on and nearby?",
                lh.name
            );
        }
    }

    Ok(())
}
