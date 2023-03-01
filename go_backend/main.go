package main

import (
	"net/http"
	"os"
	"path"

	"github.com/gin-gonic/gin"
)

func main() {
	router := gin.Default()
	router.GET("/ping", func(c *gin.Context) {
		c.JSON(http.StatusOK, gin.H{
			"message": "pong",
		})
	})
	router.POST("/upload", func(c *gin.Context) {
		file, _ := c.FormFile("file") // get file from form input name 'file'

		userDir, _ := os.UserHomeDir()

		finalPath := path.Join(userDir, file.Filename)
		c.SaveUploadedFile(file, finalPath) // save file to tmp folder in current directory

		c.String(http.StatusOK, "file: %s", file.Filename)
	})
	router.Run(":9888")
}
