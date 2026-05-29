use anyhow::{Result, bail};
use tracing::info;

use crate::{
    bluetooth::Adapter,
    lighthouse::{Lighthouse, LighthouseVersion},
};

/// Validate that the index refers to a V2 lighthouse and return it.
fn validate_lighthouse(index: usize) -> Result<Lighthouse> {
    let db = crate::storage::load()?;

    if index >= db.lighthouses.len() {
        bail!(
            "Index {} out of range (database has {} entries). Use `lighthouse-manager list` to see available indices.",
            index,
            db.lighthouses.len()
        );
    }

    let lh = &db.lighthouses[index];

    if lh.version() != LighthouseVersion::V2 {
        bail!(
            "Identify is only supported on V2 lighthouses. '{}' is a {}",
            lh.name,
            lh.version()
        );
    }

    Ok(lh.clone())
}

/// Send the identify command to a single lighthouse.
pub(super) async fn send_identify_command(adapter: &Adapter, lh: &Lighthouse) -> Result<()> {
    match tokio::time::timeout(std::time::Duration::from_secs(15), async {
        let conn = crate::bluetooth::connect_lighthouse(adapter, &lh.address).await?;
        conn.identify(lh).await?;
        conn.disconnect().await;
        Ok::<(), anyhow::Error>(())
    })
    .await
    {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(_) => bail!(
            "Timed out while trying to identify {}. Is it powered on and nearby?",
            lh.name
        ),
    }
}

/// Identify a specific V2 lighthouse by index in the database.
/// This causes the lighthouse to blink its LED for visual identification.
///
/// # Errors
///
/// Returns an error if the index is out of range, the lighthouse is not V2,
/// the adapter cannot be found, the device is not discoverable, or the identify command fails.
pub async fn run(index: usize) -> Result<()> {
    let lh = validate_lighthouse(index)?;

    info!(
        "Sending identify command to {} ({}), index [{}]...",
        lh.name, lh.address, index
    );

    let adapter = crate::bluetooth::get_adapter().await?;

    let discovered = crate::bluetooth::scan_until_predicate(&adapter, |discovered| {
        discovered.contains(&lh.address.to_lowercase())
    })
    .await?;

    if !discovered.contains(&lh.address.to_lowercase()) {
        bail!(
            "Could not observe {} ({}). Make sure it is nearby and powered on.",
            lh.name,
            lh.address
        );
    }

    send_identify_command(&adapter, &lh).await?;

    info!("{} should be blinking now.", lh.name);

    Ok(())
}
