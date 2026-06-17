use crate::drivers::storage::traits::BlockDevice;

const GPT_SIGNATURE: [u8; 8] = *b"EFI PART";
const ESP_GUID: [u8; 16] = [
    0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11,
    0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B,
];
const BASIC_DATA_GUID: [u8; 16] = [
    0xA2, 0xA0, 0xD0, 0xEB, 0xE5, 0xB9, 0x33, 0x44,
    0x87, 0xC0, 0x68, 0xB6, 0xB7, 0x26, 0x99, 0xC7,
];

pub fn is_gpt_disk(mbr: &[u8; 512]) -> bool {
    if mbr[510] != 0x55 || mbr[511] != 0xAA {
        return false;
    }
    for i in 0..4 {
        if mbr[0x1BE + i * 16 + 4] == 0xEE {
            return true;
        }
    }
    false
}

pub fn find_fat32_partition_gpt(drive: &mut dyn BlockDevice) -> Option<u64> {
    let mut sector = [0u8; 512];
    drive.read_sectors(1, 1, &mut sector).ok()?;

    if sector[0..8] != GPT_SIGNATURE {
        return None;
    }

    let header_size = u32::from_le_bytes(sector[12..16].try_into().ok()?);
    if header_size < 92 {
        return None;
    }

    let partition_entry_lba = u64::from_le_bytes(sector[72..80].try_into().ok()?);
    let num_partition_entries = u32::from_le_bytes(sector[80..84].try_into().ok()?);
    let size_of_partition_entry = u32::from_le_bytes(sector[84..88].try_into().ok()?);

    if size_of_partition_entry < 128 {
        return None;
    }

    let entry_size = size_of_partition_entry as usize;
    let num_entries = num_partition_entries.min(128) as usize;
    let entries_per_sector = 512 / entry_size;
    if entries_per_sector == 0 {
        return None;
    }

    let mut entry_buf = [0u8; 512];
    for i in 0..num_entries {
        let sector_idx = i / entries_per_sector;
        let off_in_sector = (i % entries_per_sector) * entry_size;

        if off_in_sector == 0 {
            if drive
                .read_sectors(partition_entry_lba + sector_idx as u64, 1, &mut entry_buf)
                .is_err()
            {
                return None;
            }
        }

        if off_in_sector + 16 > 512 {
            continue;
        }

        let type_guid: &[u8; 16] = match entry_buf[off_in_sector..off_in_sector + 16].try_into() {
            Ok(g) => g,
            Err(_) => continue,
        };

        let is_fat32 = type_guid == &ESP_GUID || type_guid == &BASIC_DATA_GUID;
        if !is_fat32 {
            continue;
        }

        if off_in_sector + 40 > 512 {
            continue;
        }

        let starting_lba =
            u64::from_le_bytes(entry_buf[off_in_sector + 32..off_in_sector + 40].try_into().ok()?);
        if starting_lba > 0 {
            return Some(starting_lba);
        }
    }

    None
}
