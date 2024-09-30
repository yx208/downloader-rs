import {useState} from 'react'
import './App.css'

function FileList() {
    const handleDownload = () => {
        fetch("http://localhost:6000/task/add")
            .then(res => res.json())
            .then(data => {
                console.log(data)
            })
    }

    return (
        <div className="file-list">
            <div className="file-list-item">
                <span>https://oss.xgy.tv/xgy/design/test/cool.mp4</span>
                <button onClick={handleDownload}>下载</button>
            </div>
            <div className="file-list-item">
                <span>https://oss.xgy.tv/xgy/design/test/cool.mp4</span>
                <button onClick={handleDownload}>下载</button>
            </div>
            <div className="file-list-item">
                <span>https://oss.xgy.tv/xgy/design/test/cool.mp4</span>
                <button onClick={handleDownload}>下载</button>
            </div>
        </div>
    )
}

const _temp_defaults = [{ id: "1" }, { id: "2" }];
function DownloadList() {
    const [tasks, setTasks] = useState(_temp_defaults)

    return (
        <div>
            {
                tasks.map((item) => (
                    <div key={item.id} className="download-list-item">
                        <span>C:/User/X/Downloads/filename.mp4</span>
                        <span>10MB / 100MB</span>
                        <span>Downloading</span>
                        <div>
                            <button>暂停</button>
                            <button>恢复</button>
                            <button>移除</button>
                        </div>
                    </div>
                ))
            }
        </div>
    )
}

function App() {
    return (
        <div>
            <FileList></FileList>
            <DownloadList></DownloadList>
        </div>
    )
}

export default App
