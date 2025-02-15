use std::{env,
          io::Error,
          ptr::{null,
                null_mut}};

use widestring::WideCString;
use winapi::{shared::{minwindef::{BOOL,
                                  LPDWORD},
                      ntdef::LPCWSTR,
                      winerror::*},
             um::{winbase::LookupAccountSidW,
                  winnt::{PSID,
                          PSID_NAME_USE,
                          SID_NAME_USE}}};

use super::sid::Sid;

extern "system" {
    fn LookupAccountNameW(lpSystemName: LPCWSTR,
                          lpAccountName: LPCWSTR,
                          Sid: PSID,
                          cbSid: LPDWORD,
                          ReferencedDomainName: LPCWSTR,
                          cchReferencedDomainName: LPDWORD,
                          peUse: PSID_NAME_USE)
                          -> BOOL;
}

pub struct Account {
    pub name:         String,
    pub system_name:  Option<String>,
    pub domain:       String,
    pub account_type: SID_NAME_USE,
    pub sid:          Sid,
}

impl Account {
    pub fn from_name(name: &str) -> Option<Account> { lookup_account(name, None) }

    pub fn from_name_and_system(name: &str, system_name: &str) -> Option<Account> {
        lookup_account(name, Some(system_name.to_string()))
    }

    pub fn from_sid(sid: &str) -> Option<Account> {
        #[allow(clippy::question_mark)]
        let sid = if let Ok(sid) = Sid::from_str(sid) {
            sid
        } else {
            return None;
        };
        let mut name_size: u32 = 0;
        let mut domain_size: u32 = 0;
        let ret = unsafe {
            LookupAccountSidW(null(),
                              sid.raw.as_ptr() as *mut _,
                              null_mut(),
                              &mut name_size as LPDWORD,
                              null_mut(),
                              &mut domain_size as LPDWORD,
                              null_mut())
        };
        if ret == 0 {
            match Error::last_os_error().raw_os_error().unwrap() as u32 {
                ERROR_INSUFFICIENT_BUFFER => {}
                ERROR_NONE_MAPPED => return None,
                _ => {
                    debug!("Error while looking up account for {}: {}",
                           sid.to_string().expect("to convert sid to string"),
                           Error::last_os_error());
                    return None;
                }
            }
        }

        let mut name: Vec<u16> = Vec::with_capacity(name_size as usize);
        let mut domain: Vec<u16> = Vec::with_capacity(domain_size as usize);
        let mut sid_type = 0;

        let ret = unsafe {
            LookupAccountSidW(null(),
                              sid.raw.as_ptr() as *mut _,
                              name.as_mut_ptr(),
                              &mut name_size as LPDWORD,
                              domain.as_mut_ptr(),
                              &mut domain_size as LPDWORD,
                              &mut sid_type as PSID_NAME_USE)
        };
        if ret == 0 {
            error!("Failed to retrieve name for {}: {}",
                   sid.to_string().expect("to convert sid to string"),
                   Error::last_os_error());
            return None;
        }
        unsafe {
            name.set_len(name_size as usize);
            domain.set_len(domain_size as usize);
        }
        let name = WideCString::from_vec(name).expect("valid name widestring")
                                              .to_string_lossy();
        let domain = WideCString::from_vec(domain).expect("valid domain widestring")
                                                  .to_string_lossy();
        Some(Account { name,
                       system_name: None,
                       domain,
                       account_type: sid_type,
                       sid })
    }
}

fn lookup_account(name: &str, system_name: Option<String>) -> Option<Account> {
    // if this is a machine account, strip the terminating '$'
    // LookupAccountName will return the sid of the computer account
    // given the computer name. Windows forbids usernames to match the
    // computer name
    let stripped_name =
        if name.to_lowercase() == (env::var("COMPUTERNAME").unwrap().to_lowercase() + "$") {
            &name[..name.len() - 1]
        } else {
            name
        };
    let mut sid_size: u32 = 0;
    let mut domain_size: u32 = 0;
    let wide = WideCString::from_str(stripped_name).unwrap();
    let ret = unsafe {
        LookupAccountNameW(null_mut(),
                           wide.as_ptr(),
                           null_mut(),
                           &mut sid_size as LPDWORD,
                           null_mut(),
                           &mut domain_size as LPDWORD,
                           null_mut())
    };
    if ret == 0 {
        match Error::last_os_error().raw_os_error().unwrap() as u32 {
            ERROR_INSUFFICIENT_BUFFER => {}
            ERROR_NONE_MAPPED => return None,
            _ => {
                debug!("Error while looking up account for {}: {}",
                       name,
                       Error::last_os_error());
                return None;
            }
        }
    }

    let mut sid: Vec<u8> = Vec::with_capacity(sid_size as usize);
    let mut domain: Vec<u16> = Vec::with_capacity(domain_size as usize);
    let mut sid_type: SID_NAME_USE = 0;

    let ret = unsafe {
        LookupAccountNameW(null_mut(),
                           wide.as_ptr(),
                           sid.as_mut_ptr() as PSID,
                           &mut sid_size as LPDWORD,
                           domain.as_mut_ptr(),
                           &mut domain_size as LPDWORD,
                           &mut sid_type as PSID_NAME_USE)
    };
    if ret == 0 {
        error!("Failed to retrieve SID for {}: {}",
               name,
               Error::last_os_error());
        return None;
    }
    unsafe {
        domain.set_len(domain_size as usize);
        sid.set_len(sid_size as usize);
    }
    let domain_str = WideCString::from_vec(domain).unwrap().to_string_lossy();
    Some(Account { name: name.to_string(),
                   system_name,
                   domain: domain_str,
                   account_type: sid_type,
                   sid: Sid { raw: sid } })
}

#[cfg(test)]
mod tests {
    use std::env;

    use winapi::um::winnt::{SidTypeUser,
                            SidTypeWellKnownGroup};

    use super::*;

    #[test]
    fn real_account_returns_some() { assert!(Account::from_name("Administrator").is_some()) }

    #[test]
    fn bogus_account_returns_none() { assert!(Account::from_name("bogus").is_none()) }

    #[test]
    fn user_account_returns_user_type() {
        let acct_type = Account::from_name("Administrator").unwrap().account_type;
        assert_eq!(acct_type, SidTypeUser)
    }

    #[test]
    fn local_user_account_returns_local_machine_as_domain() {
        let acct_domain = Account::from_name("Administrator").unwrap().domain;
        assert_eq!(acct_domain, env::var("COMPUTERNAME").unwrap())
    }

    #[test]
    fn well_known_group_account_returns_correct_type() {
        let acct_type = Account::from_name("Everyone").unwrap().account_type;
        assert_eq!(acct_type, SidTypeWellKnownGroup)
    }

    #[test]
    fn well_known_group_account_has_well_known_sid() {
        let sid = Account::from_name("Everyone").unwrap().sid;
        assert_eq!(sid.to_string().unwrap(), "S-1-1-0")
    }

    #[test]
    fn mixing_case_returns_same_account() {
        let current_user = env::var("USERNAME").unwrap();
        let lower_sid = Account::from_name(current_user.to_lowercase().as_str()).unwrap()
                                                                                .sid;
        let upper_sid = Account::from_name(current_user.to_uppercase().as_str()).unwrap()
                                                                                .sid;
        assert_eq!(lower_sid.to_string().unwrap(),
                   upper_sid.to_string().unwrap())
    }

    #[test]
    fn test_built_in_accounts() {
        // Check the local administrator account
        let administrator = Account::from_sid("LA").expect("Administrator account");
        assert_eq!(administrator.name, "Administrator");
        assert_eq!(administrator.domain,
                   env::var("COMPUTERNAME").expect("COMPUTERNAME env var"));
    }
}
