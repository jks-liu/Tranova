use std::{fs, io, path::PathBuf, sync::Arc};

use directories::ProjectDirs;
use parking_lot::RwLock;

use crate::models::{AppData, AppSettings, Glossary, PromptTemplate, Provider};

#[derive(Clone)]
pub struct AppStore {
    inner: Arc<StoreInner>,
}

struct StoreInner {
    path: PathBuf,
    data: RwLock<AppData>,
}

impl AppStore {
    pub fn load() -> Result<Self, io::Error> {
        let configured_directory = std::env::var_os("TRANOVA_DATA_DIR").map(PathBuf::from);
        let project_dirs = if configured_directory.is_none() {
            Some(
                ProjectDirs::from("com", "Tranova", "Tranova")
                    .ok_or_else(|| io::Error::other("unable to resolve user data directory"))?,
            )
        } else {
            None
        };
        let directory = configured_directory
            .as_deref()
            .or_else(|| project_dirs.as_ref().map(ProjectDirs::data_local_dir))
            .expect("a configured or platform data directory is always present");
        fs::create_dir_all(directory)?;
        let path = directory.join("tranova-data.json");
        let data = match fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_else(|error| {
                eprintln!("Ignoring invalid Tranova data file: {error}");
                AppData::default()
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => AppData::default(),
            Err(error) => return Err(error),
        };
        let store = Self {
            inner: Arc::new(StoreInner {
                path,
                data: RwLock::new(data),
            }),
        };
        store.persist()?;
        Ok(store)
    }

    pub fn snapshot(&self) -> AppData {
        self.inner.data.read().clone()
    }

    pub fn settings(&self) -> AppSettings {
        self.inner.data.read().settings.clone()
    }

    pub fn save_provider(&self, provider: Provider) -> Result<Provider, io::Error> {
        let provider = validate_provider(provider)?;
        let mut data = self.inner.data.write();
        upsert(&mut data.providers, provider.clone(), |value| &value.id);
        drop(data);
        self.persist()?;
        Ok(provider)
    }

    pub fn delete_provider(&self, id: &str) -> Result<(), io::Error> {
        let mut data = self.inner.data.write();
        data.providers.retain(|provider| provider.id != id);
        drop(data);
        self.persist()
    }

    pub fn save_glossary(&self, glossary: Glossary) -> Result<Glossary, io::Error> {
        if glossary.id.trim().is_empty() || glossary.name.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Glossary name and id are required",
            ));
        }
        let mut glossary = glossary;
        glossary
            .entries
            .retain(|entry| !entry.source.trim().is_empty() && !entry.target.trim().is_empty());
        let mut data = self.inner.data.write();
        upsert(&mut data.glossaries, glossary.clone(), |value| &value.id);
        drop(data);
        self.persist()?;
        Ok(glossary)
    }

    pub fn delete_glossary(&self, id: &str) -> Result<(), io::Error> {
        let mut data = self.inner.data.write();
        data.glossaries.retain(|glossary| glossary.id != id);
        drop(data);
        self.persist()
    }

    pub fn save_prompt(&self, prompt: PromptTemplate) -> Result<PromptTemplate, io::Error> {
        if prompt.id.trim().is_empty()
            || prompt.name.trim().is_empty()
            || prompt.content.trim().is_empty()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Prompt id, name and content are required",
            ));
        }
        let mut data = self.inner.data.write();
        upsert(&mut data.prompts, prompt.clone(), |value| &value.id);
        drop(data);
        self.persist()?;
        Ok(prompt)
    }

    pub fn delete_prompt(&self, id: &str) -> Result<(), io::Error> {
        let mut data = self.inner.data.write();
        data.prompts
            .retain(|prompt| prompt.id != id || id == "faithful-translation");
        drop(data);
        self.persist()
    }

    pub fn save_settings(&self, mut settings: AppSettings) -> Result<AppSettings, io::Error> {
        settings.language = match settings.language.as_str() {
            "zh-CN" => "zh-CN".to_string(),
            _ => "en".to_string(),
        };
        settings.web_host = match settings.web_host.trim() {
            "localhost" => "127.0.0.1".to_string(),
            value => value.to_string(),
        };
        let host = settings.web_host.parse::<std::net::IpAddr>().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "Web access must use a loopback IP address",
            )
        })?;
        if !host.is_loopback() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Web access is limited to this device for security; use 127.0.0.1 or ::1",
            ));
        }
        if settings.web_port < 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Web port must be between 1024 and 65535",
            ));
        }
        settings.max_chunk_chars = settings.max_chunk_chars.clamp(500, 50_000);
        self.inner.data.write().settings = settings.clone();
        self.persist()?;
        Ok(settings)
    }

    pub fn replace_glossaries(&self, glossaries: Vec<Glossary>) -> Result<(), io::Error> {
        let mut data = self.inner.data.write();
        for glossary in glossaries {
            if !glossary.id.trim().is_empty() && !glossary.name.trim().is_empty() {
                upsert(&mut data.glossaries, glossary, |value| &value.id);
            }
        }
        drop(data);
        self.persist()
    }

    pub fn replace_prompts(&self, prompts: Vec<PromptTemplate>) -> Result<(), io::Error> {
        let mut data = self.inner.data.write();
        for prompt in prompts {
            if !prompt.id.trim().is_empty()
                && !prompt.name.trim().is_empty()
                && !prompt.content.trim().is_empty()
            {
                upsert(&mut data.prompts, prompt, |value| &value.id);
            }
        }
        drop(data);
        self.persist()
    }

    fn persist(&self) -> Result<(), io::Error> {
        let content = serde_json::to_vec_pretty(&*self.inner.data.read())
            .map_err(|error| io::Error::other(error.to_string()))?;
        fs::write(&self.inner.path, content)
    }
}

fn validate_provider(mut provider: Provider) -> Result<Provider, io::Error> {
    provider.id = provider.id.trim().to_string();
    provider.name = provider.name.trim().to_string();
    provider.base_url = provider.base_url.trim().trim_end_matches('/').to_string();
    provider.model = provider.model.trim().to_string();
    if provider.id.is_empty()
        || provider.name.is_empty()
        || provider.base_url.is_empty()
        || provider.model.is_empty()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Provider id, name, API URL and model are required",
        ));
    }
    if !(provider.base_url.starts_with("http://") || provider.base_url.starts_with("https://")) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Provider URL must begin with http:// or https://",
        ));
    }
    Ok(provider)
}

fn upsert<T, F>(items: &mut Vec<T>, item: T, key: F)
where
    F: Fn(&T) -> &String,
{
    if let Some(position) = items.iter().position(|current| key(current) == key(&item)) {
        items[position] = item;
    } else {
        items.push(item);
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, sync::Arc};

    use parking_lot::RwLock;

    use super::{AppStore, StoreInner};
    use crate::models::{AppData, AppSettings};

    #[test]
    fn settings_can_be_saved_repeatedly_on_windows() {
        let directory = std::env::temp_dir().join(format!(
            "tranova-store-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        let path = directory.join("data.json");
        let store = AppStore {
            inner: Arc::new(StoreInner {
                path: path.clone(),
                data: RwLock::new(AppData::default()),
            }),
        };

        let mut settings = AppSettings {
            proxy_url: "http://127.0.0.1:8080".to_string(),
            ..AppSettings::default()
        };
        store.save_settings(settings.clone()).unwrap();
        settings.proxy_url = "socks5h://127.0.0.1:1080".to_string();
        store.save_settings(settings).unwrap();

        let saved: AppData = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved.settings.proxy_url, "socks5h://127.0.0.1:1080");
        fs::remove_file(path).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
