import chromadb
import uuid
import os
from datetime import datetime
from core.config import MEMORY_PATH

SOFTWARE_CACHE_FILE = os.path.join(MEMORY_PATH, "software_cache.json")

class AgentMemory:
    def __init__(self):
        self.client = chromadb.PersistentClient(path=MEMORY_PATH)
        self.collection = self.client.get_or_create_collection(
            name="agent_memory",
            metadata={"hnsw:space": "cosine"}
        )

    def has_software(self) -> bool:
        return os.path.exists(SOFTWARE_CACHE_FILE)

    def add(self, content: str, category: str = "chat") -> str:
        mem_id = str(uuid.uuid4())
        self.collection.add(
            documents=[content],
            ids=[mem_id],
            metadatas=[{
                "category": category,
                "time": datetime.now().isoformat()
            }]
        )
        return mem_id

    def search(self, query: str, n_results: int = 5) -> list:
        results = self.collection.query(
            query_texts=[query],
            n_results=n_results,
            include=["documents", "metadatas", "distances"]
        )
        memories = []
        if results['ids'][0]:
            for i, doc in enumerate(results['documents'][0]):
                memories.append({
                    "content": doc,
                    "meta": results['metadatas'][0][i],
                    "distance": results['distances'][0][i]
                })
        return memories

    def get_recent(self, n: int = 10) -> list:
        all_data = self.collection.get()
        return all_data

memory = AgentMemory()